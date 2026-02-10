use std::cell::{Cell as ValueCell, RefCell};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use crossterm::event::KeyCode;
use runtime::{Game, InitCtx, RuntimeEvent, UpdateCtx};
use serde::{Deserialize, Serialize};
use wasmtime::{Engine, Instance, Module, Store, TypedFunc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryType {
    Native,
    Wasm,
    Process,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    FsRead,
    FsWrite,
    Net,
    OpenUrl,
    Clipboard,
    Clock,
    Random,
    TerminalRawInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    None,
    Path(String),
    Paths(Vec<String>),
    Allowlist(Vec<String>),
    Prompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityGrant {
    pub capability: Capability,
    pub scope: Scope,
    pub decision: Decision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRequest {
    pub game_id: String,
    pub capability: Capability,
    pub scope: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDecision {
    pub decision: Decision,
    pub remembered: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPromptRequest {
    pub game_id: String,
    pub capability: Capability,
    pub scope: Scope,
    pub prompt: String,
}

pub trait CapabilityEnforcer: Send + Sync {
    fn evaluate(&self, request: &CapabilityRequest) -> Result<CapabilityDecision>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub entry_type: EntryType,
    pub entry: String,
    pub host_api: String,
    pub permissions: Vec<CapabilityGrant>,
}

pub fn validate_entry_type(
    entry_type: EntryType,
    source_scheme: &str,
    trusted_native: bool,
) -> Result<()> {
    if entry_type == EntryType::Native && !trusted_native && source_scheme != "builtin" {
        bail!("third-party native entry_type is not allowed by default policy");
    }

    if entry_type == EntryType::Process && source_scheme != "builtin" {
        bail!("process entry_type is disabled by default");
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmEntrypoint {
    pub game_id: String,
    pub wasm_path: PathBuf,
}

pub fn resolve_wasm_entrypoint(
    manifest: &PluginManifest,
    artifact_root: &Path,
) -> Result<WasmEntrypoint> {
    if manifest.entry_type != EntryType::Wasm {
        bail!(
            "manifest entry_type must be wasm for wasm launch (found {:?})",
            manifest.entry_type
        );
    }

    let entry = manifest.entry.trim();
    if entry.is_empty() {
        bail!("manifest entry must not be empty");
    }

    let entry_path = Path::new(entry);
    if entry_path.is_absolute() {
        bail!("manifest entry must be a relative path");
    }

    for component in entry_path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            _ => bail!(
                "manifest entry path escapes artifact root: {}",
                manifest.entry
            ),
        }
    }

    let wasm_path = artifact_root.join(entry_path);
    if !wasm_path.exists() {
        bail!("WASM entry file is missing: {}", wasm_path.display());
    }

    Ok(WasmEntrypoint {
        game_id: manifest.id.clone(),
        wasm_path,
    })
}

const EVENT_TICK: i32 = 0;
const EVENT_KEY_CHAR: i32 = 1;
const EVENT_KEY_UP: i32 = 2;
const EVENT_KEY_DOWN: i32 = 3;
const EVENT_KEY_LEFT: i32 = 4;
const EVENT_KEY_RIGHT: i32 = 5;
const EVENT_KEY_ENTER: i32 = 6;
const EVENT_KEY_ESC: i32 = 7;
const EVENT_RESIZE: i32 = 8;
const EVENT_FOCUS_GAINED: i32 = 9;
const EVENT_FOCUS_LOST: i32 = 10;
const EVENT_PAUSE: i32 = 11;
const EVENT_RESUME: i32 = 12;
const EVENT_OTHER_INPUT: i32 = 99;

fn leak_static(value: &str) -> &'static str {
    Box::leak(value.to_string().into_boxed_str())
}

fn normalize_guest_status(status: i32, op: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        bail!("WASM guest returned non-zero status for {op}: {status}");
    }
}

fn decode_glyph(value: i32) -> char {
    if value < 0 {
        return ' ';
    }
    let scalar = u32::try_from(value).ok();
    scalar.and_then(char::from_u32).unwrap_or(' ')
}

fn map_runtime_event(event: RuntimeEvent) -> (i32, i32, i32, Option<(u16, u16)>) {
    match event {
        RuntimeEvent::Tick { dt_ms } => (
            EVENT_TICK,
            i32::try_from(dt_ms).unwrap_or(i32::MAX),
            0,
            None,
        ),
        RuntimeEvent::Input(key) => match key.code {
            KeyCode::Char(ch) => (
                EVENT_KEY_CHAR,
                i32::try_from(u32::from(ch)).unwrap_or(0),
                0,
                None,
            ),
            KeyCode::Up => (EVENT_KEY_UP, 0, 0, None),
            KeyCode::Down => (EVENT_KEY_DOWN, 0, 0, None),
            KeyCode::Left => (EVENT_KEY_LEFT, 0, 0, None),
            KeyCode::Right => (EVENT_KEY_RIGHT, 0, 0, None),
            KeyCode::Enter => (EVENT_KEY_ENTER, 0, 0, None),
            KeyCode::Esc => (EVENT_KEY_ESC, 0, 0, None),
            _ => (EVENT_OTHER_INPUT, 0, 0, None),
        },
        RuntimeEvent::Resize { w, h } => (EVENT_RESIZE, i32::from(w), i32::from(h), Some((w, h))),
        RuntimeEvent::FocusGained => (EVENT_FOCUS_GAINED, 0, 0, None),
        RuntimeEvent::FocusLost => (EVENT_FOCUS_LOST, 0, 0, None),
        RuntimeEvent::Pause => (EVENT_PAUSE, 0, 0, None),
        RuntimeEvent::Resume => (EVENT_RESUME, 0, 0, None),
    }
}

pub struct WasmGameAdapter {
    id_static: &'static str,
    name_static: &'static str,
    #[allow(dead_code)]
    manifest: PluginManifest,
    store: RefCell<Store<()>>,
    init_func: TypedFunc<(i32, i32, i64), i32>,
    update_func: TypedFunc<(i32, i32, i32), i32>,
    cell_func: TypedFunc<(i32, i32), i32>,
    score_func: Option<TypedFunc<(), i64>>,
    finished_func: Option<TypedFunc<(), i32>>,
    width: ValueCell<u16>,
    height: ValueCell<u16>,
}

impl WasmGameAdapter {
    pub fn new(manifest: PluginManifest, artifact_root: PathBuf) -> Result<Self> {
        let entrypoint = resolve_wasm_entrypoint(&manifest, &artifact_root)?;

        let engine = Engine::default();
        let module = Module::from_file(&engine, &entrypoint.wasm_path).with_context(|| {
            format!(
                "failed to compile WASM module {}",
                entrypoint.wasm_path.display()
            )
        })?;

        let mut store = Store::new(&engine, ());
        let instance =
            Instance::new(&mut store, &module, &[]).context("failed to instantiate WASM module")?;

        let init_func = instance
            .get_typed_func::<(i32, i32, i64), i32>(&mut store, "df_init")
            .context("missing required export: df_init(width,height,seed)")?;
        let update_func = instance
            .get_typed_func::<(i32, i32, i32), i32>(&mut store, "df_update")
            .context("missing required export: df_update(kind,arg0,arg1)")?;
        let cell_func = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "df_cell")
            .context("missing required export: df_cell(x,y)")?;
        let score_func = instance
            .get_typed_func::<(), i64>(&mut store, "df_score")
            .ok();
        let finished_func = instance
            .get_typed_func::<(), i32>(&mut store, "df_finished")
            .ok();

        let id_static = leak_static(&manifest.id);
        let name_static = leak_static(&manifest.name);

        Ok(Self {
            id_static,
            name_static,
            manifest,
            store: RefCell::new(store),
            init_func,
            update_func,
            cell_func,
            score_func,
            finished_func,
            width: ValueCell::new(0),
            height: ValueCell::new(0),
        })
    }
}

impl Game for WasmGameAdapter {
    fn id(&self) -> &'static str {
        self.id_static
    }

    fn display_name(&self) -> &'static str {
        self.name_static
    }

    fn init(&mut self, ctx: &InitCtx) -> Result<()> {
        self.width.set(ctx.width);
        self.height.set(ctx.height);

        let mut store = self.store.borrow_mut();
        let status = self
            .init_func
            .call(
                &mut *store,
                (i32::from(ctx.width), i32::from(ctx.height), ctx.seed as i64),
            )
            .context("WASM guest init failed")?;
        normalize_guest_status(status, "df_init")
    }

    fn update(&mut self, event: RuntimeEvent, _ctx: &mut UpdateCtx) -> Result<()> {
        let (kind, arg0, arg1, resize) = map_runtime_event(event);
        if let Some((w, h)) = resize {
            self.width.set(w);
            self.height.set(h);
        }

        let mut store = self.store.borrow_mut();
        let status = self
            .update_func
            .call(&mut *store, (kind, arg0, arg1))
            .context("WASM guest update failed")?;
        normalize_guest_status(status, "df_update")
    }

    fn render(&self, frame: &mut runtime::Frame) {
        let render_w = usize::from(self.width.get().min(frame.width));
        let render_h = usize::from(self.height.get().min(frame.height));

        let mut store = self.store.borrow_mut();
        for y in 0..render_h {
            for x in 0..render_w {
                let x_u16 = u16::try_from(x).unwrap_or(0);
                let y_u16 = u16::try_from(y).unwrap_or(0);
                let glyph_value = self
                    .cell_func
                    .call(
                        &mut *store,
                        (
                            i32::try_from(x).unwrap_or(i32::MAX),
                            i32::try_from(y).unwrap_or(i32::MAX),
                        ),
                    )
                    .unwrap_or(32);

                frame.set(
                    x_u16,
                    y_u16,
                    runtime::Cell {
                        glyph: decode_glyph(glyph_value),
                        ..runtime::Cell::default()
                    },
                );
            }
        }
    }

    fn is_finished(&self) -> bool {
        let Some(func) = &self.finished_func else {
            return false;
        };

        let mut store = self.store.borrow_mut();
        func.call(&mut *store, ())
            .map(|value| value != 0)
            .unwrap_or(false)
    }

    fn score(&self) -> i64 {
        let Some(func) = &self.score_func else {
            return 0;
        };

        let mut store = self.store.borrow_mut();
        func.call(&mut *store, ()).unwrap_or_default()
    }

    fn reset(&mut self, ctx: &InitCtx) -> Result<()> {
        self.init(ctx)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use runtime::{Frame, Game, InitCtx, RuntimeEvent, UpdateCtx};

    use super::{
        Decision, EntryType, PluginManifest, Scope, WasmGameAdapter, resolve_wasm_entrypoint,
    };

    fn sample_manifest(entry: &str) -> PluginManifest {
        PluginManifest {
            id: "wasm-smoke".to_string(),
            name: "Wasm Smoke".to_string(),
            version: "0.1.0".to_string(),
            author: "Dark Forest".to_string(),
            entry_type: EntryType::Wasm,
            entry: entry.to_string(),
            host_api: "^0.1".to_string(),
            permissions: vec![super::CapabilityGrant {
                capability: super::Capability::TerminalRawInput,
                scope: Scope::None,
                decision: Decision::Allow,
            }],
        }
    }

    fn write_sample_wasm(root: &Path) -> anyhow::Result<()> {
        let wat = r#"
            (module
              (global $ticks (mut i32) (i32.const 0))
              (func (export "df_init") (param i32 i32 i64) (result i32)
                (i32.const 0)
              )
              (func (export "df_update") (param i32 i32 i32) (result i32)
                (local.get 0)
                (i32.const 0)
                (i32.eq)
                (if
                  (then
                    (global.get $ticks)
                    (i32.const 1)
                    (i32.add)
                    (global.set $ticks)
                  )
                )
                (i32.const 0)
              )
              (func (export "df_cell") (param i32 i32) (result i32)
                (local.get 0)
                (i32.const 0)
                (i32.eq)
                (local.get 1)
                (i32.const 0)
                (i32.eq)
                (i32.and)
                (if (result i32)
                  (then (i32.const 87))
                  (else (i32.const 32))
                )
              )
              (func (export "df_score") (result i64)
                (global.get $ticks)
                (i64.extend_i32_s)
              )
            )
        "#;

        std::fs::create_dir_all(root)?;
        let bytes = wat::parse_str(wat)?;
        std::fs::write(root.join("main.wasm"), bytes)?;
        Ok(())
    }

    #[test]
    fn resolve_wasm_entrypoint_rejects_parent_path() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let manifest = sample_manifest("../escape.wasm");
        let result = resolve_wasm_entrypoint(&manifest, temp.path());
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn wasm_adapter_smoke_renders_and_updates_score() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("game");
        write_sample_wasm(&root)?;

        let mut adapter = WasmGameAdapter::new(sample_manifest("main.wasm"), root)?;
        adapter.init(&InitCtx {
            width: 20,
            height: 8,
            seed: 7,
        })?;

        let mut update = UpdateCtx::new(20, 8);
        adapter.update(RuntimeEvent::Tick { dt_ms: 16 }, &mut update)?;

        let mut frame = Frame::new(20, 8);
        adapter.render(&mut frame);
        let first = frame.get(0, 0).unwrap_or_default();
        assert_eq!(first.glyph, 'W');
        assert!(adapter.score() >= 1);
        Ok(())
    }
}
