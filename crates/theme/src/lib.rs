use ratatui::style::{Color, Modifier, Style};

#[derive(Debug, Clone, Copy)]
pub struct ForgePalette {
    pub bg: Color,
    pub panel_bg: Color,
    pub panel_bg_2: Color,
    pub fg: Color,
    pub muted: Color,
    pub faint: Color,
    pub border: Color,
    pub accent: Color,
    pub accent_bright: Color,
    pub accent_muted: Color,
    pub rust: Color,
    pub rust_bright: Color,
    pub rust_muted: Color,
    pub danger: Color,
    pub warning: Color,
    pub success: Color,
}

pub const FORGE: ForgePalette = ForgePalette {
    bg: Color::Rgb(0x0B, 0x0F, 0x0E),
    panel_bg: Color::Rgb(0x10, 0x18, 0x16),
    panel_bg_2: Color::Rgb(0x0E, 0x14, 0x12),
    fg: Color::Rgb(0xE7, 0xEC, 0xEA),
    muted: Color::Rgb(0xA7, 0xB3, 0xAE),
    faint: Color::Rgb(0x6F, 0x7D, 0x77),
    border: Color::Rgb(0x22, 0x30, 0x2B),
    accent: Color::Rgb(0x1F, 0x8A, 0x5B),
    accent_bright: Color::Rgb(0x2F, 0xBF, 0x71),
    accent_muted: Color::Rgb(0x14, 0x5A, 0x3D),
    rust: Color::Rgb(0xB4, 0x53, 0x09),
    rust_bright: Color::Rgb(0xD9, 0x77, 0x06),
    rust_muted: Color::Rgb(0x7C, 0x2D, 0x12),
    danger: Color::Rgb(0xDC, 0x26, 0x26),
    warning: Color::Rgb(0xF5, 0x9E, 0x0B),
    success: Color::Rgb(0x2F, 0xBF, 0x71),
};

pub fn app_base_style() -> Style {
    Style::default().fg(FORGE.fg).bg(FORGE.bg)
}

pub fn panel_style() -> Style {
    Style::default().fg(FORGE.fg).bg(FORGE.panel_bg)
}

pub fn title_style() -> Style {
    Style::default()
        .fg(FORGE.accent_bright)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .fg(FORGE.accent_bright)
        .add_modifier(Modifier::BOLD)
}

pub fn muted_style() -> Style {
    Style::default().fg(FORGE.muted)
}

pub fn warning_style() -> Style {
    Style::default().fg(FORGE.warning)
}

pub fn error_style() -> Style {
    Style::default()
        .fg(FORGE.danger)
        .add_modifier(Modifier::BOLD)
}
