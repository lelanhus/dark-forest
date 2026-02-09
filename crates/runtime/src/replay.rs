use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};

use crate::{Game, RuntimeEvent, RuntimeRunner};

fn default_width() -> u16 {
    32
}

fn default_height() -> u16 {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayScenario {
    pub game_id: String,
    pub seed: u64,
    #[serde(default = "default_width")]
    pub width: u16,
    #[serde(default = "default_height")]
    pub height: u16,
    pub events: Vec<ReplayEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ReplayEvent {
    Input { key: String },
    Tick { dt_ms: u32 },
    Resize { w: u16, h: u16 },
    FocusGained,
    FocusLost,
    Pause,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayOutcome {
    pub score: i64,
    pub finished: bool,
    pub frame_hash: u64,
}

pub fn load_replay_from_path(path: impl AsRef<Path>) -> Result<ReplayScenario> {
    let path_ref = path.as_ref();
    let raw = fs::read_to_string(path_ref)
        .with_context(|| format!("failed to read replay file {}", path_ref.display()))?;
    let scenario: ReplayScenario = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse replay file {}", path_ref.display()))?;
    Ok(scenario)
}

pub fn run_replay<F>(scenario: &ReplayScenario, mut game_factory: F) -> Result<ReplayOutcome>
where
    F: FnMut(&str, u64) -> Result<Box<dyn Game + Send>>,
{
    let mut runner = RuntimeRunner::new(scenario.width, scenario.height);
    let game = game_factory(&scenario.game_id, scenario.seed)?;
    runner.start(game, scenario.seed)?;

    for event in &scenario.events {
        match event {
            ReplayEvent::Input { key } => {
                let event = RuntimeEvent::Input(parse_key(key)?);
                runner.dispatch(event)?;
            }
            ReplayEvent::Tick { dt_ms } => runner.dispatch(RuntimeEvent::Tick { dt_ms: *dt_ms })?,
            ReplayEvent::Resize { w, h } => runner.resize(*w, *h),
            ReplayEvent::FocusGained => runner.dispatch(RuntimeEvent::FocusGained)?,
            ReplayEvent::FocusLost => runner.dispatch(RuntimeEvent::FocusLost)?,
            ReplayEvent::Pause => runner.toggle_pause(),
            ReplayEvent::Resume => {
                if runner.is_paused() {
                    runner.toggle_pause();
                }
            }
        }

        if runner.is_running() {
            let _ = runner.render();
        }
        let _ = runner.take_signals();
    }

    Ok(ReplayOutcome {
        score: runner.score(),
        finished: runner.game_finished(),
        frame_hash: frame_hash(runner.current_frame()),
    })
}

fn parse_key(label: &str) -> Result<KeyEvent> {
    let code = match label {
        "Up" | "up" => KeyCode::Up,
        "Down" | "down" => KeyCode::Down,
        "Left" | "left" => KeyCode::Left,
        "Right" | "right" => KeyCode::Right,
        "Esc" | "Escape" | "escape" => KeyCode::Esc,
        "Enter" | "enter" => KeyCode::Enter,
        "Space" | "space" => KeyCode::Char(' '),
        _ => match single_char(label) {
            Some(ch) => KeyCode::Char(ch),
            None => return Err(anyhow!("unsupported replay key: {label}")),
        },
    };

    Ok(KeyEvent::new(code, KeyModifiers::empty()))
}

fn single_char(label: &str) -> Option<char> {
    let mut chars = label.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        Some(first)
    } else {
        None
    }
}

fn frame_hash(frame: &crate::Frame) -> u64 {
    let mut hasher = DefaultHasher::new();
    frame.width.hash(&mut hasher);
    frame.height.hash(&mut hasher);
    for cell in &frame.cells {
        cell.glyph.hash(&mut hasher);
        cell.attrs.bits().hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::ReplayScenario;

    #[test]
    fn replay_defaults_dimensions() -> Result<()> {
        let scenario: ReplayScenario = serde_json::from_str(
            r#"{
                "game_id":"snake-plus",
                "seed":123,
                "events":[{"type":"tick","dt_ms":16}]
            }"#,
        )?;

        assert_eq!(scenario.width, 32);
        assert_eq!(scenario.height, 20);
        Ok(())
    }
}
