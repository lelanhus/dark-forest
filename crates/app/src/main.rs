use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use content::{ContentStore, JsonContentStore};
use creator::{
    DevRequest, PackRequest, PublishRequest, TemplateInitRequest, VerifyRequest,
    game_dir_signature, init_wasm_template, pack_game, publish_to_index, run_dev_cycle,
    verify_artifact,
};
use crossterm::event::{self, Event as CrosstermEvent, KeyCode};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, terminal};
use diagnostics::{DiagnosticsSnapshot, PerfDiagnostics, detect_terminal_capabilities};
use games::builtin_catalog;
use plugin_host::{
    Capability, CapabilityDecision, CapabilityEnforcer, CapabilityRequest, Decision,
    PermissionPromptRequest, PluginManifest, Scope, WasmGameAdapter, capability_label,
    is_sensitive_capability, validate_entry_type,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use registry::{
    BuiltinRegistry, IndexRegistryProvider, RegistryProvider, parse_manifest, unpack_tarball_to_dir,
};
use runtime::{
    PerfMode, RunnerSignal, RuntimeEvent, RuntimeRunner, load_replay_from_path, run_replay,
};
use shell::{GameStatsSummary, InstalledSummary, RenderContext, Route, ShellCommand, ShellState};

#[derive(Debug)]
enum AppEvent {
    Terminal(CrosstermEvent),
    Tick,
    OperationCompleted(OperationReport),
    HotReloadDetected,
    MarketplaceCatalogLoaded(MarketplaceCatalogSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LaunchMode {
    Interactive,
    Replay { path: PathBuf },
    Operation(ContentOperation),
    Registry(RegistryCommand),
    Permissions(PermissionsCommand),
    Creator(CreatorCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RegistryCommand {
    List,
    Add { locator: String },
    Remove { locator: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PermissionsCommand {
    List {
        game_id: Option<String>,
    },
    Revoke {
        game_id: String,
        capability: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CreatorCommand {
    InitTemplate {
        game_dir: PathBuf,
        game_id: Option<String>,
        name: Option<String>,
        author: Option<String>,
        version: Option<String>,
    },
    Pack {
        game_dir: PathBuf,
        out: Option<PathBuf>,
        metadata_out: Option<PathBuf>,
    },
    VerifyArtifact {
        artifact_path: PathBuf,
        metadata_path: Option<PathBuf>,
    },
    Publish {
        artifact_path: PathBuf,
        metadata_path: Option<PathBuf>,
        index_locator: String,
        dry_run: bool,
        replace_existing: bool,
    },
    Dev {
        game_dir: PathBuf,
        out: Option<PathBuf>,
        metadata_out: Option<PathBuf>,
        index_locator: String,
        watch: bool,
        interval_ms: u64,
        dry_run_publish: bool,
        replace_existing: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ContentOperation {
    InstallLocal {
        artifact_dir: PathBuf,
        source: Option<String>,
    },
    InstallIndex {
        locator: String,
        game_id: String,
        version: Option<String>,
    },
    Update {
        game_id: String,
    },
    Rollback {
        game_id: String,
    },
    Verify {
        game_id: String,
    },
    Remove {
        game_id: String,
    },
}

impl ContentOperation {
    fn label(&self) -> String {
        match self {
            Self::InstallLocal { artifact_dir, .. } => {
                format!("install-local:{}", artifact_dir.display())
            }
            Self::InstallIndex {
                locator,
                game_id,
                version,
            } => {
                let suffix = version
                    .as_ref()
                    .map(|value| format!("@{value}"))
                    .unwrap_or_else(|| "@latest".to_string());
                format!("install-index:{locator}:{game_id}{suffix}")
            }
            Self::Update { game_id } => format!("update:{game_id}"),
            Self::Rollback { game_id } => format!("rollback:{game_id}"),
            Self::Verify { game_id } => format!("verify:{game_id}"),
            Self::Remove { game_id } => format!("remove:{game_id}"),
        }
    }
}

impl CreatorCommand {
    fn label(&self) -> &'static str {
        match self {
            Self::InitTemplate { .. } => "init-template",
            Self::Pack { .. } => "pack",
            Self::VerifyArtifact { .. } => "verify-artifact",
            Self::Publish { .. } => "publish",
            Self::Dev { .. } => "dev",
        }
    }
}

#[derive(Debug, Clone)]
struct OperationReport {
    operation: String,
    success: bool,
    message: String,
    installed_changed: bool,
}

impl OperationReport {
    fn success(operation: &ContentOperation, message: String, installed_changed: bool) -> Self {
        Self {
            operation: operation.label(),
            success: true,
            message,
            installed_changed,
        }
    }

    fn failure(operation: &ContentOperation, message: String) -> Self {
        Self {
            operation: operation.label(),
            success: false,
            message,
            installed_changed: false,
        }
    }
}

impl CreatorCommandReport {
    fn success_init_template(outcome: creator::TemplateInitOutcome) -> Self {
        Self {
            command: "init-template".to_string(),
            success: true,
            message: format!("initialized template {}", outcome.game_id),
            artifact_path: None,
            metadata_path: None,
            game_id: Some(outcome.game_id),
            version: Some(outcome.version),
            artifact_sha256: None,
            artifact_size_bytes: None,
            index_path: None,
            created_game: None,
            created_version: None,
            replaced_existing_version: None,
            dry_run: None,
            watch_mode: None,
            dev_cycles: None,
            template_game_dir: Some(outcome.game_dir),
            template_manifest_path: Some(outcome.manifest_path),
            template_entry_path: Some(outcome.entry_path),
            template_readme_path: Some(outcome.readme_path),
        }
    }

    fn success_pack(outcome: creator::PackOutcome) -> Self {
        Self {
            command: "pack".to_string(),
            success: true,
            message: format!(
                "packed {}@{}",
                outcome.metadata.game_id, outcome.metadata.version
            ),
            artifact_path: Some(outcome.artifact_path),
            metadata_path: Some(outcome.metadata_path),
            game_id: Some(outcome.metadata.game_id),
            version: Some(outcome.metadata.version),
            artifact_sha256: Some(outcome.metadata.artifact_sha256),
            artifact_size_bytes: Some(outcome.metadata.artifact_size_bytes),
            index_path: None,
            created_game: None,
            created_version: None,
            replaced_existing_version: None,
            dry_run: None,
            watch_mode: None,
            dev_cycles: None,
            template_game_dir: None,
            template_manifest_path: None,
            template_entry_path: None,
            template_readme_path: None,
        }
    }

    fn success_verify(outcome: creator::VerifyOutcome) -> Self {
        Self {
            command: "verify-artifact".to_string(),
            success: true,
            message: format!("verified {}@{}", outcome.game_id, outcome.version),
            artifact_path: Some(outcome.artifact_path),
            metadata_path: Some(outcome.metadata_path),
            game_id: Some(outcome.game_id),
            version: Some(outcome.version),
            artifact_sha256: Some(outcome.artifact_sha256),
            artifact_size_bytes: Some(outcome.artifact_size_bytes),
            index_path: None,
            created_game: None,
            created_version: None,
            replaced_existing_version: None,
            dry_run: None,
            watch_mode: None,
            dev_cycles: None,
            template_game_dir: None,
            template_manifest_path: None,
            template_entry_path: None,
            template_readme_path: None,
        }
    }

    fn success_publish(outcome: creator::PublishOutcome) -> Self {
        let message = if outcome.dry_run {
            format!("publish dry-run {}@{}", outcome.game_id, outcome.version)
        } else {
            format!("published {}@{}", outcome.game_id, outcome.version)
        };
        Self {
            command: "publish".to_string(),
            success: true,
            message,
            artifact_path: Some(outcome.artifact_path),
            metadata_path: None,
            game_id: Some(outcome.game_id),
            version: Some(outcome.version),
            artifact_sha256: None,
            artifact_size_bytes: None,
            index_path: Some(outcome.index_path),
            created_game: Some(outcome.created_game),
            created_version: Some(outcome.created_version),
            replaced_existing_version: Some(outcome.replaced_existing_version),
            dry_run: Some(outcome.dry_run),
            watch_mode: None,
            dev_cycles: None,
            template_game_dir: None,
            template_manifest_path: None,
            template_entry_path: None,
            template_readme_path: None,
        }
    }

    fn success_dev(outcome: creator::DevCycleOutcome, watch_mode: bool, dev_cycles: u64) -> Self {
        let message = if watch_mode {
            format!(
                "dev cycle {} completed for {}@{}",
                dev_cycles, outcome.publish.game_id, outcome.publish.version
            )
        } else {
            format!(
                "dev cycle completed for {}@{}",
                outcome.publish.game_id, outcome.publish.version
            )
        };
        Self {
            command: "dev".to_string(),
            success: true,
            message,
            artifact_path: Some(outcome.pack.artifact_path),
            metadata_path: Some(outcome.pack.metadata_path),
            game_id: Some(outcome.publish.game_id),
            version: Some(outcome.publish.version),
            artifact_sha256: Some(outcome.verify.artifact_sha256),
            artifact_size_bytes: Some(outcome.verify.artifact_size_bytes),
            index_path: Some(outcome.publish.index_path),
            created_game: Some(outcome.publish.created_game),
            created_version: Some(outcome.publish.created_version),
            replaced_existing_version: Some(outcome.publish.replaced_existing_version),
            dry_run: Some(outcome.publish.dry_run),
            watch_mode: Some(watch_mode),
            dev_cycles: Some(dev_cycles),
            template_game_dir: None,
            template_manifest_path: None,
            template_entry_path: None,
            template_readme_path: None,
        }
    }

    fn failure(command: &CreatorCommand, message: String) -> Self {
        Self {
            command: command.label().to_string(),
            success: false,
            message,
            artifact_path: None,
            metadata_path: None,
            game_id: None,
            version: None,
            artifact_sha256: None,
            artifact_size_bytes: None,
            index_path: None,
            created_game: None,
            created_version: None,
            replaced_existing_version: None,
            dry_run: None,
            watch_mode: None,
            dev_cycles: None,
            template_game_dir: None,
            template_manifest_path: None,
            template_entry_path: None,
            template_readme_path: None,
        }
    }
}

#[derive(Debug, Clone)]
struct RegistryCommandReport {
    command: String,
    success: bool,
    message: String,
    registry_locators: Vec<String>,
}

#[derive(Debug, Clone)]
struct PermissionsCommandReport {
    command: String,
    success: bool,
    message: String,
    grants: Vec<String>,
}

#[derive(Debug, Clone)]
struct CreatorCommandReport {
    command: String,
    success: bool,
    message: String,
    artifact_path: Option<PathBuf>,
    metadata_path: Option<PathBuf>,
    game_id: Option<String>,
    version: Option<String>,
    artifact_sha256: Option<String>,
    artifact_size_bytes: Option<u64>,
    index_path: Option<PathBuf>,
    created_game: Option<bool>,
    created_version: Option<bool>,
    replaced_existing_version: Option<bool>,
    dry_run: Option<bool>,
    watch_mode: Option<bool>,
    dev_cycles: Option<u64>,
    template_game_dir: Option<PathBuf>,
    template_manifest_path: Option<PathBuf>,
    template_entry_path: Option<PathBuf>,
    template_readme_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
struct MarketplaceCatalogSnapshot {
    games: Vec<shell::GameItem>,
    install_locators: BTreeMap<String, String>,
    warnings: Vec<String>,
}

struct AppCapabilityEnforcer {
    permissions: std::sync::Arc<std::sync::Mutex<content::PermissionsFile>>,
    session_decisions: std::sync::Arc<std::sync::Mutex<BTreeMap<(String, Capability), Decision>>>,
    prompt_queue: std::sync::Arc<std::sync::Mutex<Vec<PermissionPromptRequest>>>,
    prompt_sensitive_only: bool,
}

impl CapabilityEnforcer for AppCapabilityEnforcer {
    fn evaluate(&self, request: &CapabilityRequest) -> Result<CapabilityDecision> {
        if let Ok(session) = self.session_decisions.lock()
            && let Some(decision) = session.get(&(request.game_id.clone(), request.capability))
        {
            return Ok(CapabilityDecision {
                decision: *decision,
                remembered: false,
                reason: Some("session decision".to_string()),
            });
        }

        if let Ok(permissions) = self.permissions.lock()
            && let Some(grants) = permissions.grants.get(&request.game_id)
            && let Some(grant) = grants
                .iter()
                .find(|grant| grant.capability == request.capability && grant.remembered)
        {
            return Ok(CapabilityDecision {
                decision: grant.decision,
                remembered: true,
                reason: Some("persisted decision".to_string()),
            });
        }

        let is_low_risk = matches!(
            request.capability,
            Capability::Clock | Capability::Random | Capability::TerminalRawInput
        );
        let should_prompt = if self.prompt_sensitive_only {
            is_sensitive_capability(request.capability)
        } else {
            true
        };

        let default_decision = if should_prompt {
            if let Ok(mut queue) = self.prompt_queue.lock() {
                let already_queued = queue.iter().any(|existing| {
                    existing.game_id == request.game_id && existing.capability == request.capability
                });
                if !already_queued {
                    queue.push(PermissionPromptRequest {
                        game_id: request.game_id.clone(),
                        capability: request.capability,
                        scope: request.scope.clone(),
                        prompt: format!(
                            "Allow capability '{}' for {}?",
                            capability_label(request.capability),
                            request.game_id
                        ),
                    });
                }
            }
            Decision::Deny
        } else if is_low_risk {
            Decision::Allow
        } else {
            Decision::Deny
        };

        let reason = if default_decision == Decision::Allow {
            Some("default low-risk capability policy".to_string())
        } else if should_prompt {
            Some("default deny until explicit decision".to_string())
        } else {
            Some("capability denied by default policy".to_string())
        };

        Ok(CapabilityDecision {
            decision: default_decision,
            remembered: false,
            reason,
        })
    }
}

struct AppModel {
    shell: ShellState,
    runner: RuntimeRunner,
    store: JsonContentStore,
    settings: content::Settings,
    play_history: content::PlayHistoryMap,
    installed: content::InstalledFile,
    permissions: content::PermissionsFile,
    shared_permissions: std::sync::Arc<std::sync::Mutex<content::PermissionsFile>>,
    session_permission_decisions:
        std::sync::Arc<std::sync::Mutex<BTreeMap<(String, Capability), Decision>>>,
    permission_prompt_queue: std::sync::Arc<std::sync::Mutex<Vec<PermissionPromptRequest>>>,
    active_permission_prompt: Option<PermissionPromptRequest>,
    paused_for_permission_prompt: bool,
    best_scores: BTreeMap<String, i64>,
    current_game_id: Option<String>,
    current_game_seed: Option<u64>,
    current_game_started_at: Option<Instant>,
    seed_counter: u64,
    last_render_at: Instant,
    builtin_games: Vec<shell::GameItem>,
    marketplace_games: Vec<shell::GameItem>,
    marketplace_install_locators: BTreeMap<String, String>,
    marketplace_warnings: Vec<String>,
}

fn should_render_frame(last_render_at: Instant, now: Instant, target: Duration) -> bool {
    now.duration_since(last_render_at) >= target
}

fn should_forward_key_to_runner(
    commands: &[ShellCommand],
    route: &Route,
    overlay: Option<shell::Overlay>,
    runner_running: bool,
) -> bool {
    overlay.is_none()
        && runner_running
        && matches!(route, Route::Runner)
        && commands.iter().all(|cmd| matches!(cmd, ShellCommand::None))
}

fn parse_launch_mode(args: impl IntoIterator<Item = String>) -> Result<LaunchMode> {
    let args = args.into_iter().collect::<Vec<_>>();

    if args.is_empty() {
        return Ok(LaunchMode::Interactive);
    }

    match args[0].as_str() {
        "--help" | "-h" => {
            println!("Usage:");
            println!("  dark-forest                                  # interactive shell");
            println!("  dark-forest --replay <path>                  # run replay fixture");
            println!("  dark-forest --install-local <dir> [--source <source>]");
            println!("  dark-forest --install-index <locator> <id> [--version <ver>]");
            println!("  dark-forest --update <id>");
            println!("  dark-forest --rollback <id>");
            println!("  dark-forest --verify <id>");
            println!("  dark-forest --remove <id>");
            println!("  dark-forest --registry-list");
            println!("  dark-forest --registry-add <locator>");
            println!("  dark-forest --registry-remove <locator>");
            println!("  dark-forest --permissions-list [<game_id>]");
            println!("  dark-forest --permissions-revoke <game_id> [--capability <cap>]");
            println!(
                "  dark-forest --init-template <game_dir> [--id <game_id>] [--name <name>] [--author <author>] [--version <semver>]"
            );
            println!(
                "  dark-forest --pack <game_dir> [--out <artifact.tar.gz>] [--metadata-out <metadata.json>]"
            );
            println!(
                "  dark-forest --verify-artifact <artifact.tar.gz> [--metadata <metadata.json>]"
            );
            println!(
                "  dark-forest --publish <artifact.tar.gz> --index <locator> [--metadata <metadata.json>] [--dry-run] [--replace]"
            );
            println!(
                "  dark-forest --dev <game_dir> --index <locator> [--out <artifact.tar.gz>] [--metadata-out <metadata.json>] [--watch] [--interval-ms <ms>] [--dry-run] [--replace]"
            );
            std::process::exit(0);
        }
        "--replay" => {
            if args.len() != 2 {
                return Err(anyhow!("--replay requires exactly one path argument"));
            }
            Ok(LaunchMode::Replay {
                path: PathBuf::from(&args[1]),
            })
        }
        "--install-local" => {
            if args.len() < 2 {
                return Err(anyhow!("--install-local requires an artifact directory"));
            }
            let mut source = None;
            let mut idx = 2;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--source" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--source requires a value"));
                        }
                        source = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    other => {
                        return Err(anyhow!("unknown argument for --install-local: {other}"));
                    }
                }
            }

            Ok(LaunchMode::Operation(ContentOperation::InstallLocal {
                artifact_dir: PathBuf::from(&args[1]),
                source,
            }))
        }
        "--install-index" => {
            if args.len() < 3 {
                return Err(anyhow!(
                    "--install-index requires <locator> <id> [--version <ver>]"
                ));
            }

            let locator = args[1].clone();
            let game_id = args[2].clone();
            let mut version = None;
            let mut idx = 3;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--version" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--version requires a value"));
                        }
                        version = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    other => {
                        return Err(anyhow!("unknown argument for --install-index: {other}"));
                    }
                }
            }

            Ok(LaunchMode::Operation(ContentOperation::InstallIndex {
                locator,
                game_id,
                version,
            }))
        }
        "--update" => {
            if args.len() != 2 {
                return Err(anyhow!("--update requires exactly one game id"));
            }
            Ok(LaunchMode::Operation(ContentOperation::Update {
                game_id: args[1].clone(),
            }))
        }
        "--rollback" => {
            if args.len() != 2 {
                return Err(anyhow!("--rollback requires exactly one game id"));
            }
            Ok(LaunchMode::Operation(ContentOperation::Rollback {
                game_id: args[1].clone(),
            }))
        }
        "--verify" => {
            if args.len() != 2 {
                return Err(anyhow!("--verify requires exactly one game id"));
            }
            Ok(LaunchMode::Operation(ContentOperation::Verify {
                game_id: args[1].clone(),
            }))
        }
        "--remove" => {
            if args.len() != 2 {
                return Err(anyhow!("--remove requires exactly one game id"));
            }
            Ok(LaunchMode::Operation(ContentOperation::Remove {
                game_id: args[1].clone(),
            }))
        }
        "--registry-list" => {
            if args.len() != 1 {
                return Err(anyhow!("--registry-list takes no additional arguments"));
            }
            Ok(LaunchMode::Registry(RegistryCommand::List))
        }
        "--registry-add" => {
            if args.len() != 2 {
                return Err(anyhow!("--registry-add requires exactly one locator"));
            }
            Ok(LaunchMode::Registry(RegistryCommand::Add {
                locator: args[1].clone(),
            }))
        }
        "--registry-remove" => {
            if args.len() != 2 {
                return Err(anyhow!("--registry-remove requires exactly one locator"));
            }
            Ok(LaunchMode::Registry(RegistryCommand::Remove {
                locator: args[1].clone(),
            }))
        }
        "--permissions-list" => {
            if args.len() > 2 {
                return Err(anyhow!(
                    "--permissions-list accepts at most one optional game id"
                ));
            }
            Ok(LaunchMode::Permissions(PermissionsCommand::List {
                game_id: args.get(1).cloned(),
            }))
        }
        "--permissions-revoke" => {
            if args.len() < 2 {
                return Err(anyhow!(
                    "--permissions-revoke requires <game_id> [--capability <cap>]"
                ));
            }

            let game_id = args[1].clone();
            let mut capability = None;
            let mut idx = 2;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--capability" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--capability requires a value"));
                        }
                        capability = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    other => {
                        return Err(anyhow!(
                            "unknown argument for --permissions-revoke: {other}"
                        ));
                    }
                }
            }

            Ok(LaunchMode::Permissions(PermissionsCommand::Revoke {
                game_id,
                capability,
            }))
        }
        "--init-template" => {
            if args.len() < 2 {
                return Err(anyhow!(
                    "--init-template requires <game_dir> [--id <game_id>] [--name <name>] [--author <author>] [--version <semver>]"
                ));
            }

            let mut game_id = None;
            let mut name = None;
            let mut author = None;
            let mut version = None;
            let mut idx = 2;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--id" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--id requires a value"));
                        }
                        game_id = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    "--name" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--name requires a value"));
                        }
                        name = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    "--author" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--author requires a value"));
                        }
                        author = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    "--version" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--version requires a value"));
                        }
                        version = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    other => {
                        return Err(anyhow!("unknown argument for --init-template: {other}"));
                    }
                }
            }

            Ok(LaunchMode::Creator(CreatorCommand::InitTemplate {
                game_dir: PathBuf::from(&args[1]),
                game_id,
                name,
                author,
                version,
            }))
        }
        "--pack" => {
            if args.len() < 2 {
                return Err(anyhow!(
                    "--pack requires <game_dir> [--out <artifact.tar.gz>] [--metadata-out <metadata.json>]"
                ));
            }

            let mut out = None;
            let mut metadata_out = None;
            let mut idx = 2;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--out" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--out requires a value"));
                        }
                        out = Some(PathBuf::from(&args[idx + 1]));
                        idx += 2;
                    }
                    "--metadata-out" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--metadata-out requires a value"));
                        }
                        metadata_out = Some(PathBuf::from(&args[idx + 1]));
                        idx += 2;
                    }
                    other => {
                        return Err(anyhow!("unknown argument for --pack: {other}"));
                    }
                }
            }

            Ok(LaunchMode::Creator(CreatorCommand::Pack {
                game_dir: PathBuf::from(&args[1]),
                out,
                metadata_out,
            }))
        }
        "--verify-artifact" => {
            if args.len() < 2 {
                return Err(anyhow!(
                    "--verify-artifact requires <artifact.tar.gz> [--metadata <metadata.json>]"
                ));
            }

            let mut metadata_path = None;
            let mut idx = 2;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--metadata" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--metadata requires a value"));
                        }
                        metadata_path = Some(PathBuf::from(&args[idx + 1]));
                        idx += 2;
                    }
                    other => {
                        return Err(anyhow!("unknown argument for --verify-artifact: {other}"));
                    }
                }
            }

            Ok(LaunchMode::Creator(CreatorCommand::VerifyArtifact {
                artifact_path: PathBuf::from(&args[1]),
                metadata_path,
            }))
        }
        "--publish" => {
            if args.len() < 2 {
                return Err(anyhow!(
                    "--publish requires <artifact.tar.gz> --index <locator> [--metadata <metadata.json>] [--dry-run] [--replace]"
                ));
            }

            let mut metadata_path = None;
            let mut index_locator = None;
            let mut dry_run = false;
            let mut replace_existing = false;
            let mut idx = 2;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--metadata" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--metadata requires a value"));
                        }
                        metadata_path = Some(PathBuf::from(&args[idx + 1]));
                        idx += 2;
                    }
                    "--index" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--index requires a value"));
                        }
                        index_locator = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    "--dry-run" => {
                        dry_run = true;
                        idx += 1;
                    }
                    "--replace" => {
                        replace_existing = true;
                        idx += 1;
                    }
                    other => {
                        return Err(anyhow!("unknown argument for --publish: {other}"));
                    }
                }
            }

            let index_locator =
                index_locator.ok_or_else(|| anyhow!("--publish requires --index <locator>"))?;
            Ok(LaunchMode::Creator(CreatorCommand::Publish {
                artifact_path: PathBuf::from(&args[1]),
                metadata_path,
                index_locator,
                dry_run,
                replace_existing,
            }))
        }
        "--dev" => {
            if args.len() < 2 {
                return Err(anyhow!(
                    "--dev requires <game_dir> --index <locator> [--out <artifact.tar.gz>] [--metadata-out <metadata.json>] [--watch] [--interval-ms <ms>] [--dry-run] [--replace]"
                ));
            }

            let mut out = None;
            let mut metadata_out = None;
            let mut index_locator = None;
            let mut watch = false;
            let mut interval_ms = 1_000_u64;
            let mut dry_run_publish = false;
            let mut replace_existing = false;
            let mut idx = 2;
            while idx < args.len() {
                match args[idx].as_str() {
                    "--out" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--out requires a value"));
                        }
                        out = Some(PathBuf::from(&args[idx + 1]));
                        idx += 2;
                    }
                    "--metadata-out" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--metadata-out requires a value"));
                        }
                        metadata_out = Some(PathBuf::from(&args[idx + 1]));
                        idx += 2;
                    }
                    "--index" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--index requires a value"));
                        }
                        index_locator = Some(args[idx + 1].clone());
                        idx += 2;
                    }
                    "--watch" => {
                        watch = true;
                        idx += 1;
                    }
                    "--interval-ms" => {
                        if idx + 1 >= args.len() {
                            return Err(anyhow!("--interval-ms requires a value"));
                        }
                        interval_ms = args[idx + 1]
                            .parse::<u64>()
                            .map_err(|_| anyhow!("--interval-ms must be a positive integer"))?;
                        if interval_ms == 0 {
                            return Err(anyhow!("--interval-ms must be greater than zero"));
                        }
                        idx += 2;
                    }
                    "--dry-run" => {
                        dry_run_publish = true;
                        idx += 1;
                    }
                    "--replace" => {
                        replace_existing = true;
                        idx += 1;
                    }
                    other => {
                        return Err(anyhow!("unknown argument for --dev: {other}"));
                    }
                }
            }

            let index_locator =
                index_locator.ok_or_else(|| anyhow!("--dev requires --index <locator>"))?;
            Ok(LaunchMode::Creator(CreatorCommand::Dev {
                game_dir: PathBuf::from(&args[1]),
                out,
                metadata_out,
                index_locator,
                watch,
                interval_ms,
                dry_run_publish,
                replace_existing,
            }))
        }
        other => Err(anyhow!("unknown argument: {other}")),
    }
}

fn run_replay_cli(path: &Path) -> Result<()> {
    let scenario = load_replay_from_path(path)?;
    let outcome = run_replay(&scenario, games::instantiate)?;

    println!("replay.path={}", path.display());
    println!("replay.game_id={}", scenario.game_id);
    println!("replay.seed={}", scenario.seed);
    println!("replay.events={}", scenario.events.len());
    println!("replay.score={}", outcome.score);
    println!("replay.finished={}", outcome.finished);
    println!("replay.frame_hash={}", outcome.frame_hash);

    Ok(())
}

fn run_operation_cli(operation: ContentOperation) -> Result<()> {
    let store = JsonContentStore::create_with_default_root()?;
    let report = execute_content_operation(store.root().to_path_buf(), operation.clone())
        .unwrap_or_else(|err| OperationReport::failure(&operation, err.to_string()));

    println!("operation={}", report.operation);
    println!("success={}", report.success);
    println!("message={}", report.message);

    if report.success {
        Ok(())
    } else {
        Err(anyhow!(report.message))
    }
}

fn execute_creator_command(command: CreatorCommand) -> Result<CreatorCommandReport> {
    match command {
        CreatorCommand::InitTemplate {
            game_dir,
            game_id,
            name,
            author,
            version,
        } => {
            let outcome = init_wasm_template(&TemplateInitRequest {
                game_dir,
                game_id,
                name,
                author,
                version,
            })?;
            Ok(CreatorCommandReport::success_init_template(outcome))
        }
        CreatorCommand::Pack {
            game_dir,
            out,
            metadata_out,
        } => {
            let outcome = pack_game(&PackRequest {
                game_dir,
                out,
                metadata_out,
            })?;
            Ok(CreatorCommandReport::success_pack(outcome))
        }
        CreatorCommand::VerifyArtifact {
            artifact_path,
            metadata_path,
        } => {
            let outcome = verify_artifact(&VerifyRequest {
                artifact_path,
                metadata_path,
            })?;
            Ok(CreatorCommandReport::success_verify(outcome))
        }
        CreatorCommand::Publish {
            artifact_path,
            metadata_path,
            index_locator,
            dry_run,
            replace_existing,
        } => {
            let outcome = publish_to_index(&PublishRequest {
                artifact_path,
                metadata_path,
                index_locator,
                dry_run,
                replace_existing,
            })?;
            Ok(CreatorCommandReport::success_publish(outcome))
        }
        CreatorCommand::Dev {
            game_dir,
            out,
            metadata_out,
            index_locator,
            watch: _watch,
            interval_ms: _interval_ms,
            dry_run_publish,
            replace_existing,
        } => {
            let outcome = run_dev_cycle(&DevRequest {
                game_dir,
                out,
                metadata_out,
                index_locator,
                dry_run_publish,
                replace_existing,
            })?;
            Ok(CreatorCommandReport::success_dev(outcome, false, 1))
        }
    }
}

fn run_creator_cli(command: CreatorCommand) -> Result<()> {
    if let CreatorCommand::Dev {
        game_dir,
        out,
        metadata_out,
        index_locator,
        watch,
        interval_ms,
        dry_run_publish,
        replace_existing,
    } = &command
        && *watch
    {
        let mut cycles = 0_u64;
        let mut last_signature = game_dir_signature(game_dir)?;

        let first = run_dev_cycle(&DevRequest {
            game_dir: game_dir.clone(),
            out: out.clone(),
            metadata_out: metadata_out.clone(),
            index_locator: index_locator.clone(),
            dry_run_publish: *dry_run_publish,
            replace_existing: *replace_existing,
        })?;
        cycles += 1;
        let first_report = CreatorCommandReport::success_dev(first, true, cycles);
        print_creator_report(&first_report);

        loop {
            std::thread::sleep(Duration::from_millis(*interval_ms));
            let signature = game_dir_signature(game_dir)?;
            if signature == last_signature {
                continue;
            }
            last_signature = signature;

            let cycle = run_dev_cycle(&DevRequest {
                game_dir: game_dir.clone(),
                out: out.clone(),
                metadata_out: metadata_out.clone(),
                index_locator: index_locator.clone(),
                dry_run_publish: *dry_run_publish,
                replace_existing: *replace_existing,
            })?;
            cycles += 1;
            let report = CreatorCommandReport::success_dev(cycle, true, cycles);
            print_creator_report(&report);
        }
    }

    let report = execute_creator_command(command.clone())
        .unwrap_or_else(|err| CreatorCommandReport::failure(&command, err.to_string()));

    print_creator_report(&report);

    if report.success {
        Ok(())
    } else {
        Err(anyhow!(report.message))
    }
}

fn print_creator_report(report: &CreatorCommandReport) {
    println!("command={}", report.command);
    println!("success={}", report.success);
    println!("message={}", report.message);
    if let Some(path) = &report.artifact_path {
        println!("artifact.path={}", path.display());
    }
    if let Some(path) = &report.metadata_path {
        println!("metadata.path={}", path.display());
    }
    if let Some(game_id) = &report.game_id {
        println!("game.id={game_id}");
    }
    if let Some(version) = &report.version {
        println!("game.version={version}");
    }
    if let Some(checksum) = &report.artifact_sha256 {
        println!("artifact.sha256={checksum}");
    }
    if let Some(size) = report.artifact_size_bytes {
        println!("artifact.size_bytes={size}");
    }
    if let Some(path) = &report.index_path {
        println!("index.path={}", path.display());
    }
    if let Some(created_game) = report.created_game {
        println!("publish.created_game={created_game}");
    }
    if let Some(created_version) = report.created_version {
        println!("publish.created_version={created_version}");
    }
    if let Some(replaced) = report.replaced_existing_version {
        println!("publish.replaced_existing_version={replaced}");
    }
    if let Some(dry_run) = report.dry_run {
        println!("publish.dry_run={dry_run}");
    }
    if let Some(watch_mode) = report.watch_mode {
        println!("dev.watch={watch_mode}");
    }
    if let Some(cycles) = report.dev_cycles {
        println!("dev.cycles={cycles}");
    }
    if let Some(path) = &report.template_game_dir {
        println!("template.game_dir={}", path.display());
    }
    if let Some(path) = &report.template_manifest_path {
        println!("template.manifest_path={}", path.display());
    }
    if let Some(path) = &report.template_entry_path {
        println!("template.entry_path={}", path.display());
    }
    if let Some(path) = &report.template_readme_path {
        println!("template.readme_path={}", path.display());
    }
}

fn execute_registry_command(
    root: PathBuf,
    command: RegistryCommand,
) -> Result<RegistryCommandReport> {
    let store = JsonContentStore::new(root);
    store.ensure_layout()?;
    let mut settings = store.load_settings()?;

    let mut locators = settings
        .registries
        .iter()
        .filter(|item| item.scheme.eq_ignore_ascii_case("index"))
        .map(|item| item.locator.clone())
        .collect::<Vec<_>>();

    match command {
        RegistryCommand::List => Ok(RegistryCommandReport {
            command: "registry-list".to_string(),
            success: true,
            message: format!("{} registries configured", locators.len()),
            registry_locators: locators,
        }),
        RegistryCommand::Add { locator } => {
            if locator.trim().is_empty() {
                return Ok(RegistryCommandReport {
                    command: "registry-add".to_string(),
                    success: false,
                    message: "registry locator must not be empty".to_string(),
                    registry_locators: locators,
                });
            }

            if locators.iter().any(|entry| entry == &locator) {
                return Ok(RegistryCommandReport {
                    command: "registry-add".to_string(),
                    success: true,
                    message: format!("registry already configured: {locator}"),
                    registry_locators: locators,
                });
            }

            settings.registries.push(content::RegistryConfig {
                scheme: "index".to_string(),
                locator: locator.clone(),
            });
            store.save_settings(&settings)?;

            locators.push(locator.clone());
            Ok(RegistryCommandReport {
                command: "registry-add".to_string(),
                success: true,
                message: format!("registry added: {locator}"),
                registry_locators: locators,
            })
        }
        RegistryCommand::Remove { locator } => {
            let before_len = settings.registries.len();
            settings.registries.retain(|item| {
                !(item.scheme.eq_ignore_ascii_case("index") && item.locator == locator)
            });

            if settings.registries.len() == before_len {
                return Ok(RegistryCommandReport {
                    command: "registry-remove".to_string(),
                    success: false,
                    message: format!("registry not configured: {locator}"),
                    registry_locators: locators,
                });
            }

            store.save_settings(&settings)?;
            locators = settings
                .registries
                .iter()
                .filter(|item| item.scheme.eq_ignore_ascii_case("index"))
                .map(|item| item.locator.clone())
                .collect::<Vec<_>>();

            Ok(RegistryCommandReport {
                command: "registry-remove".to_string(),
                success: true,
                message: format!("registry removed: {locator}"),
                registry_locators: locators,
            })
        }
    }
}

fn run_registry_cli(command: RegistryCommand) -> Result<()> {
    let store = JsonContentStore::create_with_default_root()?;
    let report = execute_registry_command(store.root().to_path_buf(), command)?;

    println!("command={}", report.command);
    println!("success={}", report.success);
    println!("message={}", report.message);
    println!("registry_count={}", report.registry_locators.len());
    for (idx, locator) in report.registry_locators.iter().enumerate() {
        println!("registry[{idx}]={locator}");
    }

    if report.success {
        Ok(())
    } else {
        Err(anyhow!(report.message))
    }
}

fn parse_capability_label(raw: &str) -> Option<Capability> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "fs.read" | "fs_read" => Some(Capability::FsRead),
        "fs.write" | "fs_write" => Some(Capability::FsWrite),
        "net" => Some(Capability::Net),
        "open_url" | "open.url" | "open-url" => Some(Capability::OpenUrl),
        "clipboard" => Some(Capability::Clipboard),
        "clock" => Some(Capability::Clock),
        "random" => Some(Capability::Random),
        "terminal.raw_input" | "terminal_raw_input" => Some(Capability::TerminalRawInput),
        _ => None,
    }
}

fn decision_label(decision: Decision) -> &'static str {
    match decision {
        Decision::Allow => "allow",
        Decision::Deny => "deny",
    }
}

fn scope_label(scope: &Scope) -> String {
    match scope {
        Scope::None => "none".to_string(),
        Scope::Prompt => "prompt".to_string(),
        Scope::Path(path) => format!("path:{path}"),
        Scope::Paths(paths) => format!("paths:[{}]", paths.join(",")),
        Scope::Allowlist(entries) => format!("allowlist:[{}]", entries.join(",")),
    }
}

fn revoke_permissions(
    permissions: &mut content::PermissionsFile,
    game_id: &str,
    capability: Option<Capability>,
) -> usize {
    match capability {
        None => permissions
            .grants
            .remove(game_id)
            .map_or(0, |existing| existing.len()),
        Some(capability) => {
            let Some(grants) = permissions.grants.get_mut(game_id) else {
                return 0;
            };
            let before = grants.len();
            grants.retain(|grant| grant.capability != capability);
            let removed = before.saturating_sub(grants.len());
            if grants.is_empty() {
                permissions.grants.remove(game_id);
            }
            removed
        }
    }
}

fn flatten_permission_grants(
    permissions: &content::PermissionsFile,
    game_filter: Option<&str>,
) -> Vec<String> {
    let mut lines = Vec::new();
    for (game_id, grants) in &permissions.grants {
        if let Some(filter) = game_filter
            && filter != game_id
        {
            continue;
        }

        let mut sorted = grants.clone();
        sorted.sort_by_key(|grant| capability_label(grant.capability).to_string());
        for grant in sorted {
            lines.push(format!(
                "{game_id}|capability={}|decision={}|remembered={}|scope={}",
                capability_label(grant.capability),
                decision_label(grant.decision),
                grant.remembered,
                scope_label(&grant.scope)
            ));
        }
    }
    lines
}

fn execute_permissions_command(
    root: PathBuf,
    command: PermissionsCommand,
) -> Result<PermissionsCommandReport> {
    let store = JsonContentStore::new(root);
    store.ensure_layout()?;
    let mut permissions = store.load_permissions()?;

    match command {
        PermissionsCommand::List { game_id } => {
            let lines = flatten_permission_grants(&permissions, game_id.as_deref());
            Ok(PermissionsCommandReport {
                command: "permissions-list".to_string(),
                success: true,
                message: format!("{} permission grants", lines.len()),
                grants: lines,
            })
        }
        PermissionsCommand::Revoke {
            game_id,
            capability,
        } => {
            let parsed = match capability.as_deref() {
                Some(label) => Some(
                    parse_capability_label(label)
                        .ok_or_else(|| anyhow!("unknown capability label: {label}"))?,
                ),
                None => None,
            };

            let removed = revoke_permissions(&mut permissions, &game_id, parsed);
            if removed == 0 {
                return Ok(PermissionsCommandReport {
                    command: "permissions-revoke".to_string(),
                    success: false,
                    message: if let Some(label) = capability {
                        format!("no grants found for {game_id}:{label}")
                    } else {
                        format!("no grants found for game {game_id}")
                    },
                    grants: flatten_permission_grants(&permissions, Some(&game_id)),
                });
            }

            store.save_permissions(&permissions)?;
            Ok(PermissionsCommandReport {
                command: "permissions-revoke".to_string(),
                success: true,
                message: if let Some(label) = capability {
                    format!("revoked {removed} grant(s) for {game_id}:{label}")
                } else {
                    format!("revoked {removed} grant(s) for game {game_id}")
                },
                grants: flatten_permission_grants(&permissions, Some(&game_id)),
            })
        }
    }
}

fn run_permissions_cli(command: PermissionsCommand) -> Result<()> {
    let store = JsonContentStore::create_with_default_root()?;
    let report = execute_permissions_command(store.root().to_path_buf(), command)?;

    println!("command={}", report.command);
    println!("success={}", report.success);
    println!("message={}", report.message);
    println!("grant_count={}", report.grants.len());
    for (idx, line) in report.grants.iter().enumerate() {
        println!("grant[{idx}]={line}");
    }

    if report.success {
        Ok(())
    } else {
        Err(anyhow!(report.message))
    }
}

fn latest_played_game_id(history: &content::PlayHistoryMap) -> Option<String> {
    history
        .iter()
        .filter_map(|(id, entry)| entry.last_played_at.as_ref().map(|ts| (id, ts)))
        .max_by_key(|(_id, ts)| *ts)
        .map(|(id, _ts)| id.clone())
}

fn ensure_builtin_installed(
    mut installed: content::InstalledFile,
    games: &[shell::GameItem],
) -> content::InstalledFile {
    for game in games {
        let exists = installed.installed.iter().any(|item| item.id == game.id);
        if !exists {
            installed.installed.push(content::InstalledRecord {
                id: game.id.clone(),
                source: "builtin://dark-forest".to_string(),
                current_version: "0.1.0".to_string(),
                installed_versions: vec!["0.1.0".to_string()],
                version_checksums: BTreeMap::new(),
            });
        }
    }
    installed
}

fn read_installed_game_item(root: &Path, record: &content::InstalledRecord) -> shell::GameItem {
    let manifest_path = root
        .join("games")
        .join(&record.id)
        .join(&record.current_version)
        .join("game.json");

    if let Ok(raw) = fs::read_to_string(&manifest_path)
        && let Ok(manifest) = parse_manifest(&raw)
    {
        return shell::GameItem {
            id: manifest.id,
            name: manifest.name,
            description: format!("Installed from {}", record.source),
            tags: vec!["installed".to_string()],
        };
    }

    shell::GameItem {
        id: record.id.clone(),
        name: record.id.clone(),
        description: format!("Installed from {}", record.source),
        tags: vec!["installed".to_string()],
    }
}

fn source_scheme_from_source(source: &str) -> &str {
    source
        .split_once("://")
        .map(|(scheme, _rest)| scheme)
        .unwrap_or(source)
}

fn to_plugin_manifest(manifest: registry::Manifest) -> PluginManifest {
    PluginManifest {
        id: manifest.id,
        name: manifest.name,
        version: manifest.version,
        author: manifest.author,
        entry_type: manifest.entry_type,
        entry: manifest.entry,
        host_api: manifest.host_api,
        permissions: manifest.permissions,
    }
}

#[cfg(test)]
fn create_game_instance(
    root: &Path,
    installed: &content::InstalledFile,
    game_id: &str,
    seed: u64,
) -> Result<Box<dyn runtime::Game + Send>> {
    create_game_instance_with_enforcer(root, installed, game_id, seed, None)
}

fn create_game_instance_with_enforcer(
    root: &Path,
    installed: &content::InstalledFile,
    game_id: &str,
    seed: u64,
    enforcer: Option<std::sync::Arc<dyn CapabilityEnforcer>>,
) -> Result<Box<dyn runtime::Game + Send>> {
    if let Some(record) = installed.installed.iter().find(|item| item.id == game_id) {
        if record.source.starts_with("builtin://") {
            return games::instantiate(game_id, seed);
        }

        let source_scheme = source_scheme_from_source(&record.source);
        let artifact_dir = root
            .join("games")
            .join(game_id)
            .join(record.current_version.as_str());
        let manifest_path = artifact_dir.join("game.json");
        let raw_manifest = fs::read_to_string(&manifest_path)
            .with_context(|| format!("failed to read {}", manifest_path.display()))?;
        let parsed = parse_manifest(&raw_manifest)
            .with_context(|| format!("invalid manifest at {}", manifest_path.display()))?;
        if parsed.id != game_id {
            return Err(anyhow!(
                "manifest id mismatch for {}: expected {}, found {}",
                manifest_path.display(),
                game_id,
                parsed.id
            ));
        }

        validate_entry_type(parsed.entry_type, source_scheme, source_scheme == "builtin")?;

        return match parsed.entry_type {
            plugin_host::EntryType::Wasm => {
                let resolved_enforcer = enforcer
                    .unwrap_or_else(|| std::sync::Arc::new(plugin_host::DefaultCapabilityEnforcer));
                let game = WasmGameAdapter::with_enforcer(
                    to_plugin_manifest(parsed),
                    artifact_dir,
                    resolved_enforcer,
                )?;
                Ok(Box::new(game))
            }
            plugin_host::EntryType::Native => Err(anyhow!(
                "third-party native entry_type is not allowed by default policy"
            )),
            plugin_host::EntryType::Process => {
                Err(anyhow!("process entry_type is disabled by default"))
            }
        };
    }

    games::instantiate(game_id, seed)
}

fn load_marketplace_catalog(
    settings: &content::Settings,
    cache_root: PathBuf,
) -> MarketplaceCatalogSnapshot {
    let mut snapshot = MarketplaceCatalogSnapshot::default();

    for registry in &settings.registries {
        if !registry.scheme.eq_ignore_ascii_case("index") {
            snapshot.warnings.push(format!(
                "registry {} has unsupported scheme '{}'",
                registry.locator, registry.scheme
            ));
            continue;
        }

        if registry.locator.trim().is_empty() {
            snapshot
                .warnings
                .push("registry locator must not be empty".to_string());
            continue;
        }

        let provider = IndexRegistryProvider::new(registry.locator.clone(), cache_root.clone());
        match provider.list() {
            Ok(listings) => {
                for listing in listings {
                    if let Some(previous) = snapshot.install_locators.get(&listing.id) {
                        snapshot.warnings.push(format!(
                            "duplicate marketplace id '{}' from {} ignored (already provided by {})",
                            listing.id, registry.locator, previous
                        ));
                        continue;
                    }

                    snapshot
                        .install_locators
                        .insert(listing.id.clone(), registry.locator.clone());
                    snapshot.games.push(shell::GameItem {
                        id: listing.id,
                        name: listing.name,
                        description: listing.description,
                        tags: listing.tags,
                    });
                }
            }
            Err(err) => {
                snapshot.warnings.push(format!(
                    "registry {} load failed: {}",
                    registry.locator, err
                ));
            }
        }
    }

    snapshot
}

fn select_unpacked_artifact_root(unpack_dir: &Path) -> Result<PathBuf> {
    if unpack_dir.join("game.json").exists() {
        return Ok(unpack_dir.to_path_buf());
    }

    let mut candidates = Vec::new();
    for entry in fs::read_dir(unpack_dir)? {
        let path = entry?.path();
        if path.is_dir() && path.join("game.json").exists() {
            candidates.push(path);
        }
    }

    if candidates.len() == 1 {
        return Ok(candidates.remove(0));
    }

    Err(anyhow!(
        "unable to locate game.json in unpacked artifact {}",
        unpack_dir.display()
    ))
}

fn parse_manifest_from_artifact_dir(dir: &Path) -> Result<registry::Manifest> {
    let manifest_path = dir.join("game.json");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    parse_manifest(&raw)
}

fn install_from_index(
    store: &JsonContentStore,
    locator: &str,
    game_id: &str,
    version: Option<String>,
) -> Result<content::InstallOutcome> {
    let provider = IndexRegistryProvider::new(locator.to_string(), store.cache_dir_path());
    let artifact = provider.resolve(game_id, version.as_deref())?;
    let archive_path = provider.fetch_artifact_to_cache(&artifact)?;

    let unpack_dir = store.root().join("tmp").join(format!(
        "index-install-{}-{}-{}",
        game_id,
        artifact.version,
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));

    if unpack_dir.exists() {
        let _ = fs::remove_dir_all(&unpack_dir);
    }
    fs::create_dir_all(&unpack_dir)?;
    unpack_tarball_to_dir(&archive_path, &unpack_dir)?;
    let artifact_root = select_unpacked_artifact_root(&unpack_dir)?;

    let result = store.install_from_directory(&content::InstallRequest {
        game_id: game_id.to_string(),
        version: artifact.version,
        source: format!("index://{locator}"),
        artifact_dir: artifact_root,
        expected_sha256: artifact.checksum_sha256,
    });

    let _ = fs::remove_dir_all(&unpack_dir);
    result.map_err(|err| anyhow!(err.to_string()))
}

fn execute_content_operation(
    root: PathBuf,
    operation: ContentOperation,
) -> Result<OperationReport> {
    let store = JsonContentStore::new(root);
    store.ensure_layout()?;

    match operation.clone() {
        ContentOperation::InstallLocal {
            artifact_dir,
            source,
        } => {
            let manifest = parse_manifest_from_artifact_dir(&artifact_dir)?;
            let outcome = store
                .install_from_directory(&content::InstallRequest {
                    game_id: manifest.id.clone(),
                    version: manifest.version.clone(),
                    source: source.unwrap_or_else(|| "local://manual".to_string()),
                    artifact_dir,
                    expected_sha256: None,
                })
                .map_err(|err| anyhow!(err.to_string()))?;

            Ok(OperationReport::success(
                &operation,
                format!("installed {}@{}", outcome.game_id, outcome.version),
                true,
            ))
        }
        ContentOperation::InstallIndex {
            locator,
            game_id,
            version,
        } => {
            let outcome = install_from_index(&store, &locator, &game_id, version)?;
            Ok(OperationReport::success(
                &operation,
                format!(
                    "installed {}@{} from index",
                    outcome.game_id, outcome.version
                ),
                true,
            ))
        }
        ContentOperation::Update { game_id } => {
            let installed = store.load_installed()?;
            let record = installed
                .installed
                .iter()
                .find(|item| item.id == game_id)
                .ok_or_else(|| anyhow!("game is not installed: {game_id}"))?;

            let Some(locator) = record.source.strip_prefix("index://") else {
                return Err(anyhow!(
                    "update is currently supported only for index:// sources"
                ));
            };

            let provider = IndexRegistryProvider::new(locator.to_string(), store.cache_dir_path());
            let latest = provider.resolve(&game_id, None)?;
            if latest.version == record.current_version {
                return Ok(OperationReport::success(
                    &operation,
                    format!("{game_id} is already up to date ({})", latest.version),
                    false,
                ));
            }

            let outcome = install_from_index(&store, locator, &game_id, Some(latest.version))?;
            Ok(OperationReport::success(
                &operation,
                format!("updated {} to {}", outcome.game_id, outcome.version),
                true,
            ))
        }
        ContentOperation::Rollback { game_id } => {
            let outcome = store
                .rollback_game(&game_id)
                .map_err(|err| anyhow!(err.to_string()))?;
            Ok(OperationReport::success(
                &operation,
                format!(
                    "rolled back {} from {} to {}",
                    outcome.game_id, outcome.from_version, outcome.to_version
                ),
                true,
            ))
        }
        ContentOperation::Verify { game_id } => {
            let outcome = store
                .verify_game(&game_id)
                .map_err(|err| anyhow!(err.to_string()))?;

            if outcome.verified {
                Ok(OperationReport::success(
                    &operation,
                    format!("verified {}@{}", outcome.game_id, outcome.version),
                    false,
                ))
            } else {
                Ok(OperationReport::failure(
                    &operation,
                    format!(
                        "verification failed for {}@{} (expected {:?}, actual {})",
                        outcome.game_id,
                        outcome.version,
                        outcome.expected_sha256,
                        outcome.actual_sha256,
                    ),
                ))
            }
        }
        ContentOperation::Remove { game_id } => {
            let outcome = store
                .remove_game(&game_id)
                .map_err(|err| anyhow!(err.to_string()))?;
            Ok(OperationReport::success(
                &operation,
                format!("removed {}@{}", outcome.game_id, outcome.removed_version),
                true,
            ))
        }
    }
}

fn compute_hotload_signature(root: &Path) -> Result<u64> {
    let mut files = Vec::new();
    let installed_path = root.join("installed.json");
    if installed_path.exists() {
        files.push(installed_path);
    }

    collect_game_manifests(&root.join("games"), &mut files)?;
    files.sort();

    let mut hasher = DefaultHasher::new();
    for path in files {
        path.hash(&mut hasher);
        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed to read metadata for {}", path.display()))?;
        metadata.len().hash(&mut hasher);
        let modified = metadata.modified().ok();
        modified.hash(&mut hasher);
    }

    Ok(hasher.finish())
}

fn collect_game_manifests(dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_game_manifests(&path, output)?;
        } else if path.file_name().is_some_and(|name| name == "game.json") {
            output.push(path);
        }
    }

    Ok(())
}

impl AppModel {
    fn new() -> Result<Self> {
        let listings = builtin_catalog();
        let registry = BuiltinRegistry::new(listings.clone());
        let builtin_games = registry
            .list()?
            .into_iter()
            .map(|listing| shell::GameItem {
                id: listing.id,
                name: listing.name,
                description: listing.description,
                tags: listing.tags,
            })
            .collect::<Vec<_>>();

        let store = JsonContentStore::create_with_default_root()?;
        store.ensure_layout()?;
        let settings = store.load_settings()?;
        let play_history = store.load_play_history()?;
        let installed = ensure_builtin_installed(store.load_installed()?, &builtin_games);
        let permissions = store.load_permissions()?;
        let _ = store.save_installed(&installed);
        let mut best_scores = BTreeMap::new();

        for game in &builtin_games {
            if let Ok(scores) = store.load_high_scores(&game.id)
                && let Some(best) = scores.entries.first()
            {
                best_scores.insert(game.id.clone(), best.score);
            }
        }

        let (width, height) = terminal::size().unwrap_or((120, 40));
        let mut runner = RuntimeRunner::new(width.saturating_sub(4), height.saturating_sub(8));

        let perf_mode = match settings.performance_mode.as_str() {
            "60" => PerfMode::Fps60,
            "30" => PerfMode::Fps30,
            _ => PerfMode::Auto,
        };
        runner.set_perf_mode(perf_mode);

        let mut shell = ShellState::new(builtin_games.clone());
        shell.continue_game_id = latest_played_game_id(&play_history);
        shell.performance_mode = settings.performance_mode.clone();

        let mut model = Self {
            shell,
            runner,
            store,
            settings,
            play_history,
            installed,
            shared_permissions: std::sync::Arc::new(std::sync::Mutex::new(permissions.clone())),
            permissions,
            session_permission_decisions: std::sync::Arc::new(std::sync::Mutex::new(
                BTreeMap::new(),
            )),
            permission_prompt_queue: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            active_permission_prompt: None,
            paused_for_permission_prompt: false,
            best_scores,
            current_game_id: None,
            current_game_seed: None,
            current_game_started_at: None,
            seed_counter: Utc::now().timestamp() as u64,
            last_render_at: Instant::now(),
            builtin_games,
            marketplace_games: Vec::new(),
            marketplace_install_locators: BTreeMap::new(),
            marketplace_warnings: Vec::new(),
        };

        model.refresh_game_catalog();
        model.refresh_permission_audit_entries();
        Ok(model)
    }

    fn next_seed(&mut self) -> u64 {
        self.seed_counter = self.seed_counter.saturating_add(1);
        self.seed_counter
    }

    fn refresh_game_catalog(&mut self) {
        let mut merged = self.builtin_games.clone();

        for game in &self.marketplace_games {
            if merged.iter().any(|item| item.id == game.id) {
                continue;
            }
            merged.push(game.clone());
        }

        for record in &self.installed.installed {
            if merged.iter().any(|game| game.id == record.id) {
                continue;
            }
            merged.push(read_installed_game_item(self.store.root(), record));
        }

        self.shell.games = merged;
        self.shell.set_installed_game_ids(
            self.installed
                .installed
                .iter()
                .map(|item| item.id.clone())
                .collect(),
        );
    }

    fn sync_shared_permissions(&self) {
        if let Ok(mut shared) = self.shared_permissions.lock() {
            *shared = self.permissions.clone();
        }
    }

    fn refresh_permission_audit_entries(&mut self) {
        let mut entries = Vec::new();
        for (game_id, grants) in &self.permissions.grants {
            let mut sorted = grants.clone();
            sorted.sort_by_key(|grant| capability_label(grant.capability).to_string());
            for grant in sorted {
                entries.push(shell::PermissionAuditEntry {
                    game_id: game_id.clone(),
                    capability: capability_label(grant.capability).to_string(),
                    decision: match grant.decision {
                        Decision::Allow => "allow".to_string(),
                        Decision::Deny => "deny".to_string(),
                    },
                    remembered: grant.remembered,
                });
            }
        }
        self.shell.set_permission_audit_entries(entries);
    }

    fn maybe_show_permission_prompt(&mut self) {
        if self.active_permission_prompt.is_some() {
            return;
        }

        let next_prompt = if let Ok(mut queue) = self.permission_prompt_queue.lock() {
            if queue.is_empty() {
                None
            } else {
                Some(queue.remove(0))
            }
        } else {
            None
        };

        let Some(prompt) = next_prompt else {
            return;
        };

        self.shell
            .set_permission_prompt(Some(shell::PermissionPromptState {
                game_id: prompt.game_id.clone(),
                capability: capability_label(prompt.capability).to_string(),
                prompt: prompt.prompt.clone(),
            }));
        self.shell.overlay = Some(shell::Overlay::PermissionPrompt);
        self.active_permission_prompt = Some(prompt);

        if self.runner.is_running() && !self.runner.is_paused() {
            self.runner.toggle_pause();
            self.paused_for_permission_prompt = true;
        }
    }

    fn resolve_permission_prompt(&mut self, action: shell::PermissionPromptAction) {
        let Some(prompt) = self.active_permission_prompt.take() else {
            return;
        };

        let (decision, remember) = match action {
            shell::PermissionPromptAction::AllowOnce => (Decision::Allow, false),
            shell::PermissionPromptAction::AllowAlways => (Decision::Allow, true),
            shell::PermissionPromptAction::DenyOnce => (Decision::Deny, false),
            shell::PermissionPromptAction::DenyAlways => (Decision::Deny, true),
        };

        if let Ok(mut session) = self.session_permission_decisions.lock() {
            session.insert((prompt.game_id.clone(), prompt.capability), decision);
        }

        if remember {
            let grants = self
                .permissions
                .grants
                .entry(prompt.game_id.clone())
                .or_default();
            if let Some(existing) = grants
                .iter_mut()
                .find(|entry| entry.capability == prompt.capability)
            {
                existing.decision = decision;
                existing.scope = prompt.scope.clone();
                existing.remembered = true;
                existing.granted_at = Utc::now();
            } else {
                grants.push(content::PermissionGrant {
                    capability: prompt.capability,
                    scope: prompt.scope.clone(),
                    decision,
                    remembered: true,
                    granted_at: Utc::now(),
                });
            }

            if let Err(err) = self.store.save_permissions(&self.permissions) {
                self.shell
                    .set_error(format!("failed to save permission decision: {err}"));
            } else {
                self.sync_shared_permissions();
                self.refresh_permission_audit_entries();
            }
        }

        self.shell.set_permission_prompt(None);
        self.shell.overlay = None;
        if self.paused_for_permission_prompt && self.runner.is_paused() {
            self.runner.toggle_pause();
        }
        self.paused_for_permission_prompt = false;
        self.shell.push_notification(format!(
            "Permission decision for {}:{} -> {}",
            prompt.game_id,
            capability_label(prompt.capability),
            if decision == Decision::Allow {
                "allow"
            } else {
                "deny"
            }
        ));
    }

    fn apply_marketplace_snapshot(&mut self, snapshot: MarketplaceCatalogSnapshot) {
        self.marketplace_games = snapshot.games;
        self.marketplace_install_locators = snapshot.install_locators;
        self.refresh_game_catalog();

        for warning in &snapshot.warnings {
            if !self
                .marketplace_warnings
                .iter()
                .any(|existing| existing == warning)
            {
                self.shell
                    .push_notification(format!("Marketplace warning: {warning}"));
            }
        }
        self.marketplace_warnings = snapshot.warnings;
    }

    fn refresh_installed_state(&mut self) -> bool {
        let previous_active_version = self.current_game_id.as_ref().and_then(|id| {
            self.installed
                .installed
                .iter()
                .find(|item| &item.id == id)
                .map(|item| item.current_version.clone())
        });

        if let Ok(installed) = self.store.load_installed() {
            self.installed = installed;
            self.refresh_game_catalog();
        }

        let next_active_version = self.current_game_id.as_ref().and_then(|id| {
            self.installed
                .installed
                .iter()
                .find(|item| &item.id == id)
                .map(|item| item.current_version.clone())
        });

        previous_active_version.is_some() && previous_active_version != next_active_version
    }

    fn refresh_permissions_state(&mut self) {
        if let Ok(permissions) = self.store.load_permissions() {
            self.permissions = permissions;
            self.sync_shared_permissions();
            self.refresh_permission_audit_entries();
        }
    }

    fn cycle_performance_mode(&mut self) {
        self.settings.performance_mode = match self.settings.performance_mode.as_str() {
            "auto" => "60".to_string(),
            "60" => "30".to_string(),
            _ => "auto".to_string(),
        };

        let mode = match self.settings.performance_mode.as_str() {
            "60" => PerfMode::Fps60,
            "30" => PerfMode::Fps30,
            _ => PerfMode::Auto,
        };

        self.runner.set_perf_mode(mode);
        self.shell.performance_mode = self.settings.performance_mode.clone();
    }

    fn start_game(&mut self, game_id: &str) {
        let seed = self.next_seed();
        let enforcer = std::sync::Arc::new(AppCapabilityEnforcer {
            permissions: std::sync::Arc::clone(&self.shared_permissions),
            session_decisions: std::sync::Arc::clone(&self.session_permission_decisions),
            prompt_queue: std::sync::Arc::clone(&self.permission_prompt_queue),
            prompt_sensitive_only: self.settings.security_toggles.prompt_sensitive_only,
        });
        match create_game_instance_with_enforcer(
            self.store.root(),
            &self.installed,
            game_id,
            seed,
            Some(enforcer),
        ) {
            Ok(game) => {
                if let Err(err) = self.runner.start(game, seed) {
                    self.shell.set_error(format!("failed to start game: {err}"));
                    return;
                }
                self.shell.route = Route::Runner;
                self.shell.overlay = None;
                self.current_game_id = Some(game_id.to_string());
                self.current_game_seed = Some(seed);
                self.current_game_started_at = Some(Instant::now());
                self.last_render_at = Instant::now()
                    .checked_sub(self.runner.target_frame_duration())
                    .unwrap_or_else(Instant::now);
                self.shell.push_notification(format!("Started {game_id}"));
            }
            Err(err) => self
                .shell
                .set_error(format!("unable to start game ({game_id}): {err}")),
        }
    }

    fn stop_game(&mut self) {
        self.update_play_stats();
        self.runner.stop();
        self.shell.overlay = None;
        self.current_game_id = None;
        self.current_game_seed = None;
        self.current_game_started_at = None;
    }

    fn update_play_stats(&mut self) {
        let Some(game_id) = self.current_game_id.clone() else {
            return;
        };

        let elapsed = self
            .current_game_started_at
            .map_or(Duration::from_secs(0), |start| start.elapsed());

        let entry = self.play_history.entry(game_id.clone()).or_insert_with(|| {
            content::PlayHistoryRecord {
                game_id: game_id.clone(),
                ..content::PlayHistoryRecord::default()
            }
        });

        entry.play_count = entry.play_count.saturating_add(1);
        entry.total_play_time_seconds = entry
            .total_play_time_seconds
            .saturating_add(elapsed.as_secs());
        entry.last_played_at = Some(Utc::now());

        let mut scores = match self.store.load_high_scores(&game_id) {
            Ok(existing) => existing,
            Err(_) => content::HighScores::empty(&game_id),
        };
        scores.push_score(self.runner.score());
        if let Some(best) = scores.entries.first() {
            self.best_scores.insert(game_id.clone(), best.score);
        } else {
            self.best_scores.remove(&game_id);
        }
        self.shell.continue_game_id = latest_played_game_id(&self.play_history);

        let _ = self.store.save_high_scores(&game_id, &scores);
        let _ = self.store.save_play_history(&self.play_history);
    }

    fn handle_shell_command(&mut self, command: ShellCommand) -> bool {
        match command {
            ShellCommand::Quit => return true,
            ShellCommand::OpenRoute(route) => self.shell.route = route,
            ShellCommand::StartGame(id) => self.start_game(&id),
            ShellCommand::StopGame => self.stop_game(),
            ShellCommand::RestartGame => {
                let seed = self.next_seed();
                let _ = self.runner.restart(seed);
            }
            ShellCommand::ToggleFullscreen => self.runner.toggle_fullscreen(),
            ShellCommand::TogglePause => self.runner.toggle_pause(),
            ShellCommand::SetOverlay(overlay) => self.shell.overlay = overlay,
            ShellCommand::CyclePerformance => self.cycle_performance_mode(),
            ShellCommand::InstallSelected(_)
            | ShellCommand::UpdateInstalled(_)
            | ShellCommand::RollbackInstalled(_)
            | ShellCommand::VerifyInstalled(_)
            | ShellCommand::RemoveInstalled(_)
            | ShellCommand::RevokePermission { .. }
            | ShellCommand::ResolvePermissionPrompt(_)
            | ShellCommand::None => {}
        }

        false
    }

    fn apply_runner_signal(&mut self, signal: RunnerSignal) {
        match signal {
            RunnerSignal::Started => self.shell.push_notification("Runner started"),
            RunnerSignal::Stopped => self.shell.push_notification("Runner stopped"),
            RunnerSignal::Paused => self.shell.push_notification("Paused"),
            RunnerSignal::Resumed => self.shell.push_notification("Resumed"),
            RunnerSignal::PerfModeChanged(mode) => {
                self.shell.push_notification(format!("Perf mode: {mode:?}"))
            }
            RunnerSignal::Crashed(err) => {
                let crashed_id = self.current_game_id.clone();
                self.current_game_id = None;
                self.current_game_seed = None;
                self.current_game_started_at = None;
                self.shell.route = crashed_id
                    .map(|id| Route::GameDetail { id })
                    .unwrap_or(Route::Library);
                self.shell.set_error(format!("Game crashed: {err}"));
            }
        }
    }

    fn apply_operation_report(&mut self, report: OperationReport) {
        if !self.shell.progress.is_empty() {
            let _ = self.shell.progress.remove(0);
        }

        if report.success {
            self.shell.push_notification(report.message);
            self.refresh_permissions_state();
            if report.installed_changed {
                let active_changed = self.refresh_installed_state();
                if active_changed {
                    self.shell
                        .push_notification("Active game content changed. Restart to apply.");
                }
            }
        } else {
            self.shell
                .set_error(format!("{} failed: {}", report.operation, report.message));
        }
    }

    fn render_context(&self) -> RenderContext {
        let mode_label = match self.runner.perf_mode() {
            PerfMode::Auto => format!(
                "mode:auto fps:{} avg:{:.2}ms",
                self.runner.auto_target_fps(),
                self.runner.average_render_ms()
            ),
            PerfMode::Fps60 => "mode:60".to_string(),
            PerfMode::Fps30 => "mode:30".to_string(),
        };
        let mut game_stats = BTreeMap::new();
        for game in &self.shell.games {
            let history = self.play_history.get(&game.id);
            game_stats.insert(
                game.id.clone(),
                GameStatsSummary {
                    play_count: history.map_or(0, |item| item.play_count),
                    best_score: self.best_scores.get(&game.id).copied(),
                    last_played_at: history.and_then(|item| {
                        item.last_played_at
                            .as_ref()
                            .map(chrono::DateTime::<Utc>::to_rfc3339)
                    }),
                },
            );
        }
        let installed = self
            .installed
            .installed
            .iter()
            .map(|item| {
                (
                    item.id.clone(),
                    InstalledSummary {
                        current_version: item.current_version.clone(),
                        source: item.source.clone(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>();

        RenderContext {
            current_game: self.current_game_id.clone(),
            runner_frame: if self.runner.is_running() {
                Some(self.runner.current_frame().clone())
            } else {
                None
            },
            runner_paused: self.runner.is_paused(),
            runner_fullscreen: self.runner.is_fullscreen(),
            perf_summary: mode_label,
            game_stats,
            installed,
        }
    }

    fn diagnostics_snapshot(&self) -> DiagnosticsSnapshot {
        DiagnosticsSnapshot {
            terminal: detect_terminal_capabilities(),
            perf: PerfDiagnostics {
                target_fps: self.runner.auto_target_fps(),
                render_ms_avg: self.runner.average_render_ms(),
                mode: format!("{:?}", self.runner.perf_mode()),
            },
        }
    }

    fn maybe_render_runner_frame(&mut self, now: Instant) {
        if !self.runner.is_running() {
            return;
        }

        let target = self.runner.target_frame_duration();
        if should_render_frame(self.last_render_at, now, target) {
            let _ = self.runner.render();
            self.last_render_at = now;
        }
    }
}

fn enqueue_operation(
    shell: &mut ShellState,
    tx: &tokio::sync::mpsc::Sender<ContentOperation>,
    operation: ContentOperation,
) {
    let label = operation.label();
    match tx.try_send(operation) {
        Ok(()) => {
            shell.progress.push(format!("Running: {label}"));
            if shell.progress.len() > 5 {
                let _ = shell.progress.remove(0);
            }
        }
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            shell.set_error("operation queue is full; try again");
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            shell.set_error("operation worker unavailable");
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    match parse_launch_mode(std::env::args().skip(1))? {
        LaunchMode::Interactive => run().await,
        LaunchMode::Replay { path } => run_replay_cli(&path),
        LaunchMode::Operation(operation) => run_operation_cli(operation),
        LaunchMode::Registry(command) => run_registry_cli(command),
        LaunchMode::Permissions(command) => run_permissions_cli(command),
        LaunchMode::Creator(command) => run_creator_cli(command),
    }
}

async fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    stdout.execute(EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let result = run_loop(&mut terminal).await;

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    let mut model = AppModel::new()?;

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<AppEvent>(256);
    let (operation_tx, mut operation_rx) = tokio::sync::mpsc::channel::<ContentOperation>(32);
    let running = Arc::new(AtomicBool::new(true));

    let input_running = Arc::clone(&running);
    let input_tx = event_tx.clone();
    let input_task = tokio::task::spawn_blocking(move || {
        while input_running.load(Ordering::SeqCst) {
            if event::poll(Duration::from_millis(20)).unwrap_or(false)
                && let Ok(evt) = event::read()
            {
                match input_tx.try_send(AppEvent::Terminal(evt)) {
                    Ok(()) => {}
                    Err(tokio::sync::mpsc::error::TrySendError::Full(AppEvent::Terminal(
                        CrosstermEvent::Key(key),
                    ))) if matches!(
                        key.code,
                        KeyCode::Up
                            | KeyCode::Down
                            | KeyCode::Left
                            | KeyCode::Right
                            | KeyCode::Char('j')
                            | KeyCode::Char('k')
                    ) => {}
                    Err(tokio::sync::mpsc::error::TrySendError::Full(event)) => {
                        let _ = input_tx.blocking_send(event);
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                }
            }
        }
    });

    let tick_running = Arc::clone(&running);
    let tick_tx = event_tx.clone();
    let tick_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(16));
        while tick_running.load(Ordering::SeqCst) {
            interval.tick().await;
            let _ = tick_tx.try_send(AppEvent::Tick);
        }
    });

    let operation_running = Arc::clone(&running);
    let operation_event_tx = event_tx.clone();
    let operation_root = model.store.root().to_path_buf();
    let operation_task = tokio::spawn(async move {
        while operation_running.load(Ordering::SeqCst) {
            let Some(operation) = operation_rx.recv().await else {
                break;
            };

            let operation_for_error = operation.clone();
            let root = operation_root.clone();
            let result =
                tokio::task::spawn_blocking(move || execute_content_operation(root, operation))
                    .await;

            let report = match result {
                Ok(Ok(report)) => report,
                Ok(Err(err)) => OperationReport::failure(&operation_for_error, err.to_string()),
                Err(join_err) => OperationReport::failure(
                    &operation_for_error,
                    format!("operation task failed: {join_err}"),
                ),
            };

            let _ = operation_event_tx
                .send(AppEvent::OperationCompleted(report))
                .await;
        }
    });

    let watch_running = Arc::clone(&running);
    let watch_tx = event_tx.clone();
    let watch_root = model.store.root().to_path_buf();
    let watch_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let mut last_signature = compute_hotload_signature(&watch_root).ok();

        while watch_running.load(Ordering::SeqCst) {
            interval.tick().await;
            let next_signature = compute_hotload_signature(&watch_root).ok();
            if next_signature.is_some() && next_signature != last_signature {
                last_signature = next_signature;
                let _ = watch_tx.try_send(AppEvent::HotReloadDetected);
            }
        }
    });

    let marketplace_running = Arc::clone(&running);
    let marketplace_tx = event_tx.clone();
    let marketplace_settings = model.settings.clone();
    let marketplace_cache_root = model.store.cache_dir_path();
    let marketplace_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        while marketplace_running.load(Ordering::SeqCst) {
            interval.tick().await;
            let settings = marketplace_settings.clone();
            let cache_root = marketplace_cache_root.clone();
            let snapshot = match tokio::task::spawn_blocking(move || {
                load_marketplace_catalog(&settings, cache_root)
            })
            .await
            {
                Ok(snapshot) => snapshot,
                Err(join_err) => MarketplaceCatalogSnapshot {
                    games: Vec::new(),
                    install_locators: BTreeMap::new(),
                    warnings: vec![format!("marketplace refresh task failed: {join_err}")],
                },
            };

            let _ = marketplace_tx
                .send(AppEvent::MarketplaceCatalogLoaded(snapshot))
                .await;
        }
    });

    let mut should_quit = false;
    while !should_quit {
        if let Some(event) = event_rx.recv().await {
            match event {
                AppEvent::Tick => {
                    let _ = model.runner.dispatch(RuntimeEvent::Tick { dt_ms: 16 });
                    model.maybe_render_runner_frame(Instant::now());

                    for signal in model.runner.take_signals() {
                        model.apply_runner_signal(signal);
                    }

                    model.maybe_show_permission_prompt();

                    if model.runner.game_finished() {
                        let previous_game_id = model.current_game_id.clone();
                        model.stop_game();
                        model.shell.route = Route::GameDetail {
                            id: previous_game_id.unwrap_or_else(|| "unknown".to_string()),
                        };
                    }
                }
                AppEvent::OperationCompleted(report) => {
                    model.apply_operation_report(report);
                }
                AppEvent::HotReloadDetected => {
                    if model.refresh_installed_state() {
                        model.shell.push_notification(
                            "Installed content changed. Restart active game to apply.",
                        );
                    }
                    model.refresh_permissions_state();
                }
                AppEvent::MarketplaceCatalogLoaded(snapshot) => {
                    model.apply_marketplace_snapshot(snapshot);
                }
                AppEvent::Terminal(event) => match event {
                    CrosstermEvent::Key(key) => {
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(event::KeyModifiers::CONTROL)
                        {
                            should_quit = true;
                            continue;
                        }

                        let commands = model.shell.handle_key(
                            key,
                            model.runner.is_running(),
                            model.runner.is_paused(),
                        );
                        let forwarded_to_runner = should_forward_key_to_runner(
                            &commands,
                            &model.shell.route,
                            model.shell.overlay,
                            model.runner.is_running(),
                        );

                        for command in commands {
                            match command {
                                ShellCommand::InstallSelected(id) => {
                                    if model.installed.installed.iter().any(|item| item.id == id) {
                                        continue;
                                    }

                                    if let Some(locator) =
                                        model.marketplace_install_locators.get(&id).cloned()
                                    {
                                        enqueue_operation(
                                            &mut model.shell,
                                            &operation_tx,
                                            ContentOperation::InstallIndex {
                                                locator,
                                                game_id: id,
                                                version: None,
                                            },
                                        );
                                    } else {
                                        model.shell.set_error(format!(
                                            "install source unavailable for game: {id}"
                                        ));
                                    }
                                }
                                ShellCommand::UpdateInstalled(id) => enqueue_operation(
                                    &mut model.shell,
                                    &operation_tx,
                                    ContentOperation::Update { game_id: id },
                                ),
                                ShellCommand::RollbackInstalled(id) => enqueue_operation(
                                    &mut model.shell,
                                    &operation_tx,
                                    ContentOperation::Rollback { game_id: id },
                                ),
                                ShellCommand::VerifyInstalled(id) => enqueue_operation(
                                    &mut model.shell,
                                    &operation_tx,
                                    ContentOperation::Verify { game_id: id },
                                ),
                                ShellCommand::RemoveInstalled(id) => enqueue_operation(
                                    &mut model.shell,
                                    &operation_tx,
                                    ContentOperation::Remove { game_id: id },
                                ),
                                ShellCommand::RevokePermission {
                                    game_id,
                                    capability,
                                } => {
                                    let parsed =
                                        capability.as_deref().and_then(parse_capability_label);
                                    let removed = revoke_permissions(
                                        &mut model.permissions,
                                        &game_id,
                                        parsed,
                                    );
                                    if removed == 0 {
                                        model.shell.set_error(if let Some(capability) = capability {
                                            format!(
                                                "no permission grants to revoke for {game_id}:{capability}"
                                            )
                                        } else {
                                            format!("no permission grants to revoke for {game_id}")
                                        });
                                        continue;
                                    }

                                    if let Err(err) =
                                        model.store.save_permissions(&model.permissions)
                                    {
                                        model.shell.set_error(format!(
                                            "failed to persist permission revoke: {err}"
                                        ));
                                        continue;
                                    }

                                    model.sync_shared_permissions();
                                    model.refresh_permissions_state();
                                    model.shell.push_notification(if let Some(capability) = capability {
                                        format!(
                                            "Revoked {removed} permission grant(s) for {game_id}:{capability}"
                                        )
                                    } else {
                                        format!(
                                            "Revoked {removed} permission grant(s) for {game_id}"
                                        )
                                    });
                                }
                                ShellCommand::ResolvePermissionPrompt(action) => {
                                    model.resolve_permission_prompt(action);
                                }
                                other => {
                                    if model.handle_shell_command(other) {
                                        should_quit = true;
                                    }
                                }
                            }
                        }

                        if forwarded_to_runner {
                            let _ = model.runner.dispatch(RuntimeEvent::Input(key));
                        }
                    }
                    CrosstermEvent::Resize(w, h) => {
                        let runner_w = w.saturating_sub(4).max(20);
                        let runner_h = h.saturating_sub(8).max(10);
                        model.runner.resize(runner_w, runner_h);
                    }
                    CrosstermEvent::FocusLost => {
                        let _ = model.runner.dispatch(RuntimeEvent::FocusLost);
                        if model.runner.is_running() && !model.runner.is_paused() {
                            model.runner.toggle_pause();
                        }
                    }
                    CrosstermEvent::FocusGained => {
                        let _ = model.runner.dispatch(RuntimeEvent::FocusGained);
                    }
                    _ => {}
                },
            }

            terminal.draw(|frame| {
                shell::render(frame, &model.shell, &model.render_context());
            })?;
        }
    }

    running.store(false, Ordering::SeqCst);
    let _ = input_task.await;
    let _ = tick_task.await;
    let _ = watch_task.await;
    let _ = marketplace_task.await;
    let _ = operation_task.await;

    model.update_play_stats();
    model.store.save_settings(&model.settings)?;
    model.store.save_play_history(&model.play_history)?;
    model.store.save_installed(&model.installed)?;
    model.store.save_permissions(&model.permissions)?;

    let snapshot = model.diagnostics_snapshot();
    tracing::info!(
        width = snapshot.terminal.width,
        height = snapshot.terminal.height,
        truecolor = snapshot.terminal.truecolor,
        target_fps = snapshot.perf.target_fps,
        avg_ms = snapshot.perf.render_ms_avg,
        mode = snapshot.perf.mode,
        "shutdown diagnostics"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use content::ContentStore;
    use plugin_host::{Capability, CapabilityEnforcer, CapabilityRequest, Decision, Scope};
    use shell::{Overlay, Route, ShellCommand};

    use super::{
        AppCapabilityEnforcer, ContentOperation, CreatorCommand, LaunchMode, PermissionsCommand,
        RegistryCommand, compute_hotload_signature, create_game_instance,
        execute_content_operation, execute_creator_command, execute_permissions_command,
        execute_registry_command, load_marketplace_catalog, parse_launch_mode,
        should_forward_key_to_runner, should_render_frame,
    };

    fn write_sample_wasm(path: &std::path::Path) -> anyhow::Result<()> {
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

        let bytes = wat::parse_str(wat)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }

    fn make_enforcer(prompt_sensitive_only: bool) -> AppCapabilityEnforcer {
        AppCapabilityEnforcer {
            permissions: std::sync::Arc::new(std::sync::Mutex::new(content::PermissionsFile {
                schema_version: content::CURRENT_SCHEMA_VERSION,
                grants: std::collections::BTreeMap::new(),
            })),
            session_decisions: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::BTreeMap::new(),
            )),
            prompt_queue: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            prompt_sensitive_only,
        }
    }

    #[test]
    fn render_scheduler_waits_until_target_duration() {
        let last = Instant::now();
        let now = last + Duration::from_millis(10);
        assert!(!should_render_frame(last, now, Duration::from_millis(16)));
    }

    #[test]
    fn render_scheduler_allows_render_when_target_elapsed() {
        let last = Instant::now();
        let now = last + Duration::from_millis(16);
        assert!(should_render_frame(last, now, Duration::from_millis(16)));
    }

    #[test]
    fn capability_enforcer_prompts_sensitive_capabilities_by_default() {
        let enforcer = make_enforcer(true);
        let request = CapabilityRequest {
            game_id: "remote-wasm".to_string(),
            capability: Capability::Net,
            scope: Scope::Prompt,
        };

        let decision = enforcer
            .evaluate(&request)
            .expect("capability evaluation should succeed");
        assert_eq!(decision.decision, Decision::Deny);
        let queue = enforcer
            .prompt_queue
            .lock()
            .expect("queue lock should succeed");
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn capability_enforcer_allows_low_risk_when_prompt_sensitive_only() {
        let enforcer = make_enforcer(true);
        let request = CapabilityRequest {
            game_id: "remote-wasm".to_string(),
            capability: Capability::Clock,
            scope: Scope::None,
        };

        let decision = enforcer
            .evaluate(&request)
            .expect("capability evaluation should succeed");
        assert_eq!(decision.decision, Decision::Allow);
        let queue = enforcer
            .prompt_queue
            .lock()
            .expect("queue lock should succeed");
        assert!(queue.is_empty());
    }

    #[test]
    fn capability_enforcer_prompts_low_risk_when_prompt_all_enabled() {
        let enforcer = make_enforcer(false);
        let request = CapabilityRequest {
            game_id: "remote-wasm".to_string(),
            capability: Capability::Clock,
            scope: Scope::None,
        };

        let decision = enforcer
            .evaluate(&request)
            .expect("capability evaluation should succeed");
        assert_eq!(decision.decision, Decision::Deny);
        let queue = enforcer
            .prompt_queue
            .lock()
            .expect("queue lock should succeed");
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn parses_replay_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--replay".to_string(),
            "fixtures/replays/snake-seed-12345.json".to_string(),
        ])
        .expect("replay mode should parse");
        assert_eq!(
            mode,
            LaunchMode::Replay {
                path: PathBuf::from("fixtures/replays/snake-seed-12345.json")
            }
        );
    }

    #[test]
    fn parses_install_local_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--install-local".to_string(),
            "/tmp/game".to_string(),
            "--source".to_string(),
            "local://fixtures".to_string(),
        ])
        .expect("install-local mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Operation(ContentOperation::InstallLocal {
                artifact_dir: PathBuf::from("/tmp/game"),
                source: Some("local://fixtures".to_string()),
            })
        );
    }

    #[test]
    fn parses_install_index_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--install-index".to_string(),
            "file:///tmp/index.json".to_string(),
            "snake-plus".to_string(),
            "--version".to_string(),
            "0.2.0".to_string(),
        ])
        .expect("install-index mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Operation(ContentOperation::InstallIndex {
                locator: "file:///tmp/index.json".to_string(),
                game_id: "snake-plus".to_string(),
                version: Some("0.2.0".to_string()),
            })
        );
    }

    #[test]
    fn parses_pack_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--pack".to_string(),
            "/tmp/game".to_string(),
            "--out".to_string(),
            "/tmp/out/sample-game-1.2.3.tar.gz".to_string(),
            "--metadata-out".to_string(),
            "/tmp/out/sample-game-1.2.3.metadata.json".to_string(),
        ])
        .expect("pack mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Creator(CreatorCommand::Pack {
                game_dir: PathBuf::from("/tmp/game"),
                out: Some(PathBuf::from("/tmp/out/sample-game-1.2.3.tar.gz")),
                metadata_out: Some(PathBuf::from("/tmp/out/sample-game-1.2.3.metadata.json")),
            })
        );
    }

    #[test]
    fn parses_verify_artifact_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--verify-artifact".to_string(),
            "/tmp/out/sample-game-1.2.3.tar.gz".to_string(),
            "--metadata".to_string(),
            "/tmp/out/sample-game-1.2.3.metadata.json".to_string(),
        ])
        .expect("verify-artifact mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Creator(CreatorCommand::VerifyArtifact {
                artifact_path: PathBuf::from("/tmp/out/sample-game-1.2.3.tar.gz"),
                metadata_path: Some(PathBuf::from("/tmp/out/sample-game-1.2.3.metadata.json")),
            })
        );
    }

    #[test]
    fn parses_publish_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--publish".to_string(),
            "/tmp/out/sample-game-1.2.3.tar.gz".to_string(),
            "--index".to_string(),
            "file:///tmp/index.json".to_string(),
            "--metadata".to_string(),
            "/tmp/out/sample-game-1.2.3.metadata.json".to_string(),
        ])
        .expect("publish mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Creator(CreatorCommand::Publish {
                artifact_path: PathBuf::from("/tmp/out/sample-game-1.2.3.tar.gz"),
                metadata_path: Some(PathBuf::from("/tmp/out/sample-game-1.2.3.metadata.json")),
                index_locator: "file:///tmp/index.json".to_string(),
                dry_run: false,
                replace_existing: false,
            })
        );
    }

    #[test]
    fn parses_publish_launch_mode_with_dry_run_and_replace() {
        let mode = parse_launch_mode(vec![
            "--publish".to_string(),
            "/tmp/out/sample-game-1.2.3.tar.gz".to_string(),
            "--index".to_string(),
            "file:///tmp/index.json".to_string(),
            "--dry-run".to_string(),
            "--replace".to_string(),
        ])
        .expect("publish mode with flags should parse");

        assert_eq!(
            mode,
            LaunchMode::Creator(CreatorCommand::Publish {
                artifact_path: PathBuf::from("/tmp/out/sample-game-1.2.3.tar.gz"),
                metadata_path: None,
                index_locator: "file:///tmp/index.json".to_string(),
                dry_run: true,
                replace_existing: true,
            })
        );
    }

    #[test]
    fn parses_dev_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--dev".to_string(),
            "/tmp/game".to_string(),
            "--index".to_string(),
            "file:///tmp/index.json".to_string(),
            "--out".to_string(),
            "/tmp/out/sample-game-1.2.3.tar.gz".to_string(),
            "--metadata-out".to_string(),
            "/tmp/out/sample-game-1.2.3.metadata.json".to_string(),
        ])
        .expect("dev mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Creator(CreatorCommand::Dev {
                game_dir: PathBuf::from("/tmp/game"),
                out: Some(PathBuf::from("/tmp/out/sample-game-1.2.3.tar.gz")),
                metadata_out: Some(PathBuf::from("/tmp/out/sample-game-1.2.3.metadata.json")),
                index_locator: "file:///tmp/index.json".to_string(),
                watch: false,
                interval_ms: 1_000,
                dry_run_publish: false,
                replace_existing: false,
            })
        );
    }

    #[test]
    fn parses_dev_watch_launch_mode_with_flags() {
        let mode = parse_launch_mode(vec![
            "--dev".to_string(),
            "/tmp/game".to_string(),
            "--index".to_string(),
            "file:///tmp/index.json".to_string(),
            "--watch".to_string(),
            "--interval-ms".to_string(),
            "250".to_string(),
            "--dry-run".to_string(),
            "--replace".to_string(),
        ])
        .expect("dev watch mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Creator(CreatorCommand::Dev {
                game_dir: PathBuf::from("/tmp/game"),
                out: None,
                metadata_out: None,
                index_locator: "file:///tmp/index.json".to_string(),
                watch: true,
                interval_ms: 250,
                dry_run_publish: true,
                replace_existing: true,
            })
        );
    }

    #[test]
    fn parses_init_template_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--init-template".to_string(),
            "/tmp/my-game".to_string(),
            "--id".to_string(),
            "my-game".to_string(),
            "--name".to_string(),
            "My Game".to_string(),
            "--author".to_string(),
            "Dark Forest".to_string(),
            "--version".to_string(),
            "0.3.0".to_string(),
        ])
        .expect("init-template mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Creator(CreatorCommand::InitTemplate {
                game_dir: PathBuf::from("/tmp/my-game"),
                game_id: Some("my-game".to_string()),
                name: Some("My Game".to_string()),
                author: Some("Dark Forest".to_string()),
                version: Some("0.3.0".to_string()),
            })
        );
    }

    #[test]
    fn pack_launch_mode_requires_game_dir() {
        let err = parse_launch_mode(vec!["--pack".to_string()]);
        assert!(err.is_err());
    }

    #[test]
    fn pack_launch_mode_rejects_unknown_argument() {
        let err = parse_launch_mode(vec![
            "--pack".to_string(),
            "/tmp/game".to_string(),
            "--unknown".to_string(),
        ]);
        assert!(err.is_err());
    }

    #[test]
    fn verify_artifact_launch_mode_requires_artifact_path() {
        let err = parse_launch_mode(vec!["--verify-artifact".to_string()]);
        assert!(err.is_err());
    }

    #[test]
    fn verify_artifact_launch_mode_rejects_unknown_argument() {
        let err = parse_launch_mode(vec![
            "--verify-artifact".to_string(),
            "/tmp/out/sample-game-1.2.3.tar.gz".to_string(),
            "--nope".to_string(),
        ]);
        assert!(err.is_err());
    }

    #[test]
    fn publish_launch_mode_requires_artifact_path() {
        let err = parse_launch_mode(vec!["--publish".to_string()]);
        assert!(err.is_err());
    }

    #[test]
    fn publish_launch_mode_requires_index_locator() {
        let err = parse_launch_mode(vec![
            "--publish".to_string(),
            "/tmp/out/sample-game-1.2.3.tar.gz".to_string(),
        ]);
        assert!(err.is_err());
    }

    #[test]
    fn publish_launch_mode_rejects_unknown_argument() {
        let err = parse_launch_mode(vec![
            "--publish".to_string(),
            "/tmp/out/sample-game-1.2.3.tar.gz".to_string(),
            "--index".to_string(),
            "file:///tmp/index.json".to_string(),
            "--nope".to_string(),
        ]);
        assert!(err.is_err());
    }

    #[test]
    fn dev_launch_mode_requires_game_dir() {
        let err = parse_launch_mode(vec!["--dev".to_string()]);
        assert!(err.is_err());
    }

    #[test]
    fn dev_launch_mode_requires_index_locator() {
        let err = parse_launch_mode(vec!["--dev".to_string(), "/tmp/game".to_string()]);
        assert!(err.is_err());
    }

    #[test]
    fn dev_launch_mode_rejects_invalid_interval() {
        let err = parse_launch_mode(vec![
            "--dev".to_string(),
            "/tmp/game".to_string(),
            "--index".to_string(),
            "file:///tmp/index.json".to_string(),
            "--interval-ms".to_string(),
            "not-a-number".to_string(),
        ]);
        assert!(err.is_err());
    }

    #[test]
    fn dev_launch_mode_rejects_unknown_argument() {
        let err = parse_launch_mode(vec![
            "--dev".to_string(),
            "/tmp/game".to_string(),
            "--index".to_string(),
            "file:///tmp/index.json".to_string(),
            "--nope".to_string(),
        ]);
        assert!(err.is_err());
    }

    #[test]
    fn init_template_launch_mode_requires_game_dir() {
        let err = parse_launch_mode(vec!["--init-template".to_string()]);
        assert!(err.is_err());
    }

    #[test]
    fn init_template_launch_mode_rejects_unknown_argument() {
        let err = parse_launch_mode(vec![
            "--init-template".to_string(),
            "/tmp/my-game".to_string(),
            "--unknown".to_string(),
        ]);
        assert!(err.is_err());
    }

    #[test]
    fn init_template_launch_mode_rejects_missing_option_value() {
        let err = parse_launch_mode(vec![
            "--init-template".to_string(),
            "/tmp/my-game".to_string(),
            "--id".to_string(),
        ]);
        assert!(err.is_err());
    }

    #[test]
    fn init_template_command_creates_scaffold_and_report_paths() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = temp.path().join("my-game");

        let report = execute_creator_command(CreatorCommand::InitTemplate {
            game_dir: game_dir.clone(),
            game_id: Some("my-game".to_string()),
            name: Some("My Game".to_string()),
            author: Some("Dark Forest".to_string()),
            version: Some("0.5.0".to_string()),
        })?;

        assert!(report.success);
        assert_eq!(report.command, "init-template");
        assert_eq!(report.game_id.as_deref(), Some("my-game"));
        assert_eq!(report.version.as_deref(), Some("0.5.0"));
        let report_game_dir = report
            .template_game_dir
            .expect("template game dir should be present");
        assert_eq!(report_game_dir, game_dir);
        let manifest_path = report
            .template_manifest_path
            .expect("template manifest path should be present");
        let entry_path = report
            .template_entry_path
            .expect("template entry path should be present");
        let readme_path = report
            .template_readme_path
            .expect("template readme path should be present");
        assert!(manifest_path.exists());
        assert!(entry_path.exists());
        assert!(readme_path.exists());
        Ok(())
    }

    #[test]
    fn init_template_command_fails_for_nonempty_directory() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let game_dir = temp.path().join("existing-game");
        std::fs::create_dir_all(&game_dir)?;
        std::fs::write(game_dir.join("occupied.txt"), b"already here")?;

        let result = execute_creator_command(CreatorCommand::InitTemplate {
            game_dir,
            game_id: Some("existing-game".to_string()),
            name: Some("Existing Game".to_string()),
            author: Some("Dark Forest".to_string()),
            version: Some("0.1.0".to_string()),
        });
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn parses_remove_launch_mode() {
        let mode = parse_launch_mode(vec!["--remove".to_string(), "snake-plus".to_string()])
            .expect("remove mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Operation(ContentOperation::Remove {
                game_id: "snake-plus".to_string(),
            })
        );
    }

    #[test]
    fn parses_registry_list_launch_mode() {
        let mode = parse_launch_mode(vec!["--registry-list".to_string()])
            .expect("registry-list mode should parse");

        assert_eq!(mode, LaunchMode::Registry(RegistryCommand::List));
    }

    #[test]
    fn parses_registry_add_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--registry-add".to_string(),
            "file:///tmp/index.json".to_string(),
        ])
        .expect("registry-add mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Registry(RegistryCommand::Add {
                locator: "file:///tmp/index.json".to_string(),
            })
        );
    }

    #[test]
    fn parses_registry_remove_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--registry-remove".to_string(),
            "file:///tmp/index.json".to_string(),
        ])
        .expect("registry-remove mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Registry(RegistryCommand::Remove {
                locator: "file:///tmp/index.json".to_string(),
            })
        );
    }

    #[test]
    fn parses_permissions_list_launch_mode() {
        let mode = parse_launch_mode(vec!["--permissions-list".to_string()])
            .expect("permissions-list mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Permissions(PermissionsCommand::List { game_id: None })
        );
    }

    #[test]
    fn parses_permissions_revoke_launch_mode() {
        let mode = parse_launch_mode(vec![
            "--permissions-revoke".to_string(),
            "remote-wasm".to_string(),
            "--capability".to_string(),
            "net".to_string(),
        ])
        .expect("permissions-revoke mode should parse");

        assert_eq!(
            mode,
            LaunchMode::Permissions(PermissionsCommand::Revoke {
                game_id: "remote-wasm".to_string(),
                capability: Some("net".to_string()),
            })
        );
    }

    #[test]
    fn rejects_unknown_launch_argument() {
        let error = parse_launch_mode(vec!["--nope".to_string()]);
        assert!(error.is_err());
    }

    #[test]
    fn overlay_blocks_runner_input_forwarding() {
        let commands = vec![ShellCommand::None];
        assert!(!should_forward_key_to_runner(
            &commands,
            &Route::Runner,
            Some(Overlay::RunnerQuitConfirm),
            true
        ));
    }

    #[test]
    fn unhandled_runner_key_without_overlay_is_forwarded() {
        let commands = vec![ShellCommand::None];
        assert!(should_forward_key_to_runner(
            &commands,
            &Route::Runner,
            None,
            true
        ));
    }

    #[test]
    fn hotload_signature_changes_when_installed_changes() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let store = content::JsonContentStore::new(temp.path().to_path_buf());
        store.ensure_layout()?;

        let before = compute_hotload_signature(store.root())?;
        let mut installed = store.load_installed()?;
        installed.installed.push(content::InstalledRecord {
            id: "sample-game".to_string(),
            source: "local://fixture".to_string(),
            current_version: "0.1.0".to_string(),
            installed_versions: vec!["0.1.0".to_string()],
            version_checksums: std::collections::BTreeMap::new(),
        });
        store.save_installed(&installed)?;
        let after = compute_hotload_signature(store.root())?;

        assert_ne!(before, after);
        Ok(())
    }

    #[test]
    fn rollback_operation_effects_current_pointer() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();
        let artifact_v1 = root.join("artifact-v1");
        let artifact_v2 = root.join("artifact-v2");
        std::fs::create_dir_all(&artifact_v1)?;
        std::fs::create_dir_all(&artifact_v2)?;

        std::fs::write(
            artifact_v1.join("game.json"),
            serde_json::json!({
                "id": "snake-plus",
                "name": "Snake+",
                "version": "0.1.0",
                "author": "Dark Forest",
                "entry_type": "wasm",
                "entry": "main.wasm",
                "host_api": "^0.1",
                "permissions": ["terminal.raw_input"]
            })
            .to_string(),
        )?;
        std::fs::write(artifact_v1.join("main.wasm"), b"v1")?;

        std::fs::write(
            artifact_v2.join("game.json"),
            serde_json::json!({
                "id": "snake-plus",
                "name": "Snake+",
                "version": "0.2.0",
                "author": "Dark Forest",
                "entry_type": "wasm",
                "entry": "main.wasm",
                "host_api": "^0.1",
                "permissions": ["terminal.raw_input"]
            })
            .to_string(),
        )?;
        std::fs::write(artifact_v2.join("main.wasm"), b"v2")?;

        let install1 = execute_content_operation(
            root.clone(),
            ContentOperation::InstallLocal {
                artifact_dir: artifact_v1,
                source: Some("local://fixture".to_string()),
            },
        )?;
        assert!(install1.success);

        let install2 = execute_content_operation(
            root.clone(),
            ContentOperation::InstallLocal {
                artifact_dir: artifact_v2,
                source: Some("local://fixture".to_string()),
            },
        )?;
        assert!(install2.success);

        let rollback = execute_content_operation(
            root.clone(),
            ContentOperation::Rollback {
                game_id: "snake-plus".to_string(),
            },
        )?;
        assert!(rollback.success);

        let store = content::JsonContentStore::new(root);
        assert_eq!(
            store.read_current_pointer("snake-plus"),
            Some("0.1.0".to_string())
        );
        Ok(())
    }

    #[test]
    fn remove_operation_deletes_installed_record_and_payload() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();
        let artifact_v1 = root.join("artifact-v1");
        std::fs::create_dir_all(&artifact_v1)?;

        std::fs::write(
            artifact_v1.join("game.json"),
            serde_json::json!({
                "id": "snake-plus",
                "name": "Snake+",
                "version": "0.1.0",
                "author": "Dark Forest",
                "entry_type": "wasm",
                "entry": "main.wasm",
                "host_api": "^0.1",
                "permissions": ["terminal.raw_input"]
            })
            .to_string(),
        )?;
        std::fs::write(artifact_v1.join("main.wasm"), b"v1")?;

        let install = execute_content_operation(
            root.clone(),
            ContentOperation::InstallLocal {
                artifact_dir: artifact_v1,
                source: Some("local://fixture".to_string()),
            },
        )?;
        assert!(install.success);

        let remove = execute_content_operation(
            root.clone(),
            ContentOperation::Remove {
                game_id: "snake-plus".to_string(),
            },
        )?;
        assert!(remove.success);

        let store = content::JsonContentStore::new(root.clone());
        let installed = store.load_installed()?;
        assert!(
            !installed
                .installed
                .iter()
                .any(|item| item.id == "snake-plus")
        );
        assert!(!root.join("games/snake-plus").exists());
        Ok(())
    }

    #[test]
    fn registry_add_is_idempotent() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let first = execute_registry_command(
            root.clone(),
            RegistryCommand::Add {
                locator: "file:///tmp/index.json".to_string(),
            },
        )?;
        assert!(first.success);

        let second = execute_registry_command(
            root.clone(),
            RegistryCommand::Add {
                locator: "file:///tmp/index.json".to_string(),
            },
        )?;
        assert!(second.success);
        assert_eq!(second.registry_locators, vec!["file:///tmp/index.json"]);

        let listed = execute_registry_command(root, RegistryCommand::List)?;
        assert!(listed.success);
        assert_eq!(listed.registry_locators, vec!["file:///tmp/index.json"]);
        Ok(())
    }

    #[test]
    fn registry_remove_missing_is_failure() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let removed = execute_registry_command(
            root,
            RegistryCommand::Remove {
                locator: "file:///tmp/missing-index.json".to_string(),
            },
        )?;
        assert!(!removed.success);
        assert_eq!(
            removed.message,
            "registry not configured: file:///tmp/missing-index.json"
        );
        Ok(())
    }

    #[test]
    fn registry_list_preserves_order() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        execute_registry_command(
            root.clone(),
            RegistryCommand::Add {
                locator: "file:///tmp/first.json".to_string(),
            },
        )?;
        execute_registry_command(
            root.clone(),
            RegistryCommand::Add {
                locator: "file:///tmp/second.json".to_string(),
            },
        )?;

        let listed = execute_registry_command(root, RegistryCommand::List)?;
        assert!(listed.success);
        assert_eq!(
            listed.registry_locators,
            vec!["file:///tmp/first.json", "file:///tmp/second.json"]
        );
        Ok(())
    }

    #[test]
    fn permissions_list_is_stable_and_ordered() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();
        let store = content::JsonContentStore::new(root.clone());
        store.ensure_layout()?;

        let mut permissions = store.load_permissions()?;
        permissions.grants.insert(
            "game-b".to_string(),
            vec![content::PermissionGrant {
                capability: Capability::Net,
                scope: Scope::Prompt,
                decision: Decision::Deny,
                remembered: true,
                granted_at: chrono::Utc::now(),
            }],
        );
        permissions.grants.insert(
            "game-a".to_string(),
            vec![content::PermissionGrant {
                capability: Capability::Clock,
                scope: Scope::None,
                decision: Decision::Allow,
                remembered: true,
                granted_at: chrono::Utc::now(),
            }],
        );
        store.save_permissions(&permissions)?;

        let report = execute_permissions_command(root, PermissionsCommand::List { game_id: None })?;
        assert!(report.success);
        assert_eq!(report.grants.len(), 2);
        assert!(report.grants[0].starts_with("game-a|"));
        assert!(report.grants[1].starts_with("game-b|"));
        Ok(())
    }

    #[test]
    fn permissions_revoke_missing_is_failure() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let report = execute_permissions_command(
            root,
            PermissionsCommand::Revoke {
                game_id: "ghost".to_string(),
                capability: Some("net".to_string()),
            },
        )?;
        assert!(!report.success);
        assert_eq!(report.message, "no grants found for ghost:net");
        Ok(())
    }

    #[test]
    fn marketplace_catalog_loads_and_reports_errors() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let index_path = root.join("index.json");
        std::fs::write(
            &index_path,
            serde_json::json!({
                "schema_version": 1,
                "games": [{
                    "id": "remote-snake",
                    "name": "Remote Snake",
                    "description": "Remote listing",
                    "tags": ["arcade", "remote"],
                    "author": "Remote Team",
                    "versions": [{
                        "version": "1.0.0",
                        "artifact": "remote-snake-1.0.0.tar.gz"
                    }]
                }]
            })
            .to_string(),
        )?;

        let settings = content::Settings {
            registries: vec![
                content::RegistryConfig {
                    scheme: "index".to_string(),
                    locator: index_path.to_string_lossy().to_string(),
                },
                content::RegistryConfig {
                    scheme: "index".to_string(),
                    locator: root.join("missing.json").to_string_lossy().to_string(),
                },
            ],
            ..content::Settings::default()
        };

        let snapshot = load_marketplace_catalog(&settings, root.join("cache"));
        assert!(
            snapshot.games.iter().any(|game| game.id == "remote-snake"),
            "expected marketplace game to load"
        );
        assert!(
            snapshot
                .warnings
                .iter()
                .any(|item| item.contains("missing.json")),
            "expected warning for failed registry fetch"
        );
        Ok(())
    }

    #[test]
    fn marketplace_catalog_deduplicates_by_first_registry() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let first_index = root.join("first.json");
        std::fs::write(
            &first_index,
            serde_json::json!({
                "schema_version": 1,
                "games": [{
                    "id": "dup-game",
                    "name": "First Name",
                    "description": "From first",
                    "tags": ["first"],
                    "author": "A",
                    "versions": [{
                        "version": "1.0.0",
                        "artifact": "dup-game-1.0.0.tar.gz"
                    }]
                }]
            })
            .to_string(),
        )?;

        let second_index = root.join("second.json");
        std::fs::write(
            &second_index,
            serde_json::json!({
                "schema_version": 1,
                "games": [{
                    "id": "dup-game",
                    "name": "Second Name",
                    "description": "From second",
                    "tags": ["second"],
                    "author": "B",
                    "versions": [{
                        "version": "2.0.0",
                        "artifact": "dup-game-2.0.0.tar.gz"
                    }]
                }]
            })
            .to_string(),
        )?;

        let settings = content::Settings {
            registries: vec![
                content::RegistryConfig {
                    scheme: "index".to_string(),
                    locator: first_index.to_string_lossy().to_string(),
                },
                content::RegistryConfig {
                    scheme: "index".to_string(),
                    locator: second_index.to_string_lossy().to_string(),
                },
            ],
            ..content::Settings::default()
        };

        let snapshot = load_marketplace_catalog(&settings, root.join("cache"));
        assert_eq!(snapshot.games.len(), 1);
        assert_eq!(snapshot.games[0].name, "First Name");
        assert_eq!(
            snapshot.install_locators.get("dup-game"),
            Some(&first_index.to_string_lossy().to_string())
        );
        assert!(
            snapshot
                .warnings
                .iter()
                .any(|item| item.contains("duplicate marketplace id")),
            "expected duplicate warning"
        );
        Ok(())
    }

    #[test]
    fn launch_resolver_starts_installed_wasm_game() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let store = content::JsonContentStore::new(root.clone());
        store.ensure_layout()?;

        let game_id = "remote-wasm";
        let version = "1.0.0";
        let game_root = root.join("games").join(game_id).join(version);
        std::fs::create_dir_all(&game_root)?;
        std::fs::write(
            game_root.join("game.json"),
            serde_json::json!({
                "id": game_id,
                "name": "Remote WASM",
                "version": version,
                "author": "Remote Team",
                "entry_type": "wasm",
                "entry": "main.wasm",
                "host_api": "^0.1",
                "permissions": ["terminal.raw_input"]
            })
            .to_string(),
        )?;
        write_sample_wasm(&game_root.join("main.wasm"))?;

        let installed = content::InstalledFile {
            schema_version: content::CURRENT_SCHEMA_VERSION,
            installed: vec![content::InstalledRecord {
                id: game_id.to_string(),
                source: "index://file:///tmp/index.json".to_string(),
                current_version: version.to_string(),
                installed_versions: vec![version.to_string()],
                version_checksums: std::collections::BTreeMap::new(),
            }],
        };
        store.save_installed(&installed)?;

        let mut game = create_game_instance(&root, &installed, game_id, 42)?;
        game.init(&runtime::InitCtx {
            width: 20,
            height: 8,
            seed: 42,
        })?;
        let mut update = runtime::UpdateCtx::new(20, 8);
        game.update(runtime::RuntimeEvent::Tick { dt_ms: 16 }, &mut update)?;

        let mut frame = runtime::Frame::new(20, 8);
        game.render(&mut frame);
        assert_eq!(frame.get(0, 0).unwrap_or_default().glyph, 'W');
        Ok(())
    }

    #[test]
    fn launch_resolver_rejects_third_party_native_entry() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().to_path_buf();

        let store = content::JsonContentStore::new(root.clone());
        store.ensure_layout()?;

        let game_id = "remote-native";
        let version = "1.0.0";
        let game_root = root.join("games").join(game_id).join(version);
        std::fs::create_dir_all(&game_root)?;
        std::fs::write(
            game_root.join("game.json"),
            serde_json::json!({
                "id": game_id,
                "name": "Remote Native",
                "version": version,
                "author": "Remote Team",
                "entry_type": "native",
                "entry": "game.bin",
                "host_api": "^0.1",
                "permissions": ["terminal.raw_input"]
            })
            .to_string(),
        )?;
        std::fs::write(game_root.join("game.bin"), b"native")?;

        let installed = content::InstalledFile {
            schema_version: content::CURRENT_SCHEMA_VERSION,
            installed: vec![content::InstalledRecord {
                id: game_id.to_string(),
                source: "index://file:///tmp/index.json".to_string(),
                current_version: version.to_string(),
                installed_versions: vec![version.to_string()],
                version_checksums: std::collections::BTreeMap::new(),
            }],
        };
        store.save_installed(&installed)?;

        let err = match create_game_instance(&root, &installed, game_id, 42) {
            Ok(_) => panic!("expected third-party native launch to fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string()
                .contains("third-party native entry_type is not allowed"),
            "unexpected error: {err}"
        );
        Ok(())
    }
}
