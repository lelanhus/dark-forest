use crossterm::terminal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalCapabilities {
    pub width: u16,
    pub height: u16,
    pub truecolor: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfDiagnostics {
    pub target_fps: u16,
    pub render_ms_avg: f32,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsSnapshot {
    pub terminal: TerminalCapabilities,
    pub perf: PerfDiagnostics,
}

pub fn detect_terminal_capabilities() -> TerminalCapabilities {
    let (width, height) = terminal::size().unwrap_or((80, 24));
    let truecolor = std::env::var("COLORTERM")
        .map(|v| v.to_lowercase().contains("truecolor") || v.to_lowercase().contains("24bit"))
        .unwrap_or(false);

    TerminalCapabilities {
        width,
        height,
        truecolor,
    }
}
