use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use runtime::Frame;
use theme::{FORGE, app_base_style, muted_style, panel_style, selected_style, title_style};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Home,
    Library,
    Installed,
    Settings,
    GameDetail { id: String },
    Runner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    CommandPalette,
    Search,
    Help,
    Notifications,
    Progress,
    ErrorDetail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameItem {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ShellState {
    pub route: Route,
    pub overlay: Option<Overlay>,
    pub notifications: Vec<String>,
    pub progress: Vec<String>,
    pub last_error: Option<String>,
    pub games: Vec<GameItem>,
    pub home_index: usize,
    pub list_index: usize,
    pub settings_index: usize,
    pub performance_mode: String,
}

#[derive(Debug, Clone)]
pub enum ShellCommand {
    Quit,
    OpenRoute(Route),
    StartGame(String),
    StopGame,
    RestartGame,
    ToggleFullscreen,
    TogglePause,
    SetOverlay(Option<Overlay>),
    CyclePerformance,
    None,
}

#[derive(Debug, Clone)]
pub struct RenderContext {
    pub current_game: Option<String>,
    pub runner_frame: Option<Frame>,
    pub runner_paused: bool,
    pub runner_fullscreen: bool,
    pub perf_summary: String,
}

impl Default for RenderContext {
    fn default() -> Self {
        Self {
            current_game: None,
            runner_frame: None,
            runner_paused: false,
            runner_fullscreen: false,
            perf_summary: "fps:auto".to_string(),
        }
    }
}

impl ShellState {
    #[must_use]
    pub fn new(games: Vec<GameItem>) -> Self {
        Self {
            route: Route::Home,
            overlay: None,
            notifications: Vec::new(),
            progress: Vec::new(),
            last_error: None,
            games,
            home_index: 0,
            list_index: 0,
            settings_index: 0,
            performance_mode: "auto".to_string(),
        }
    }

    pub fn push_notification(&mut self, value: impl Into<String>) {
        self.notifications.push(value.into());
        if self.notifications.len() > 5 {
            let _ = self.notifications.remove(0);
        }
    }

    pub fn set_error(&mut self, err: impl Into<String>) {
        self.last_error = Some(err.into());
        self.overlay = Some(Overlay::ErrorDetail);
    }

    pub fn handle_key(&mut self, key: KeyEvent, has_running_game: bool) -> Vec<ShellCommand> {
        if let Some(active_overlay) = self.overlay {
            return self.handle_overlay_key(key, active_overlay);
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('q') => return vec![ShellCommand::Quit],
                KeyCode::Char('k') => {
                    return vec![ShellCommand::SetOverlay(Some(Overlay::CommandPalette))];
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('/') => vec![ShellCommand::SetOverlay(Some(Overlay::Search))],
            KeyCode::Char('?') => vec![ShellCommand::SetOverlay(Some(Overlay::Help))],
            _ => self.handle_route_key(key, has_running_game),
        }
    }

    fn handle_overlay_key(&mut self, key: KeyEvent, overlay: Overlay) -> Vec<ShellCommand> {
        match key.code {
            KeyCode::Esc => vec![ShellCommand::SetOverlay(None)],
            KeyCode::Char('n') if overlay == Overlay::Help => {
                vec![ShellCommand::SetOverlay(Some(Overlay::Notifications))]
            }
            _ => vec![ShellCommand::None],
        }
    }

    fn handle_route_key(&mut self, key: KeyEvent, has_running_game: bool) -> Vec<ShellCommand> {
        match self.route.clone() {
            Route::Home => self.handle_home_key(key),
            Route::Library | Route::Installed => self.handle_library_key(key),
            Route::Settings => self.handle_settings_key(key),
            Route::GameDetail { id } => self.handle_detail_key(key, id),
            Route::Runner => self.handle_runner_key(key, has_running_game),
        }
    }

    fn handle_home_key(&mut self, key: KeyEvent) -> Vec<ShellCommand> {
        const HOME_ITEMS: usize = 4;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.home_index = (self.home_index + 1) % HOME_ITEMS;
                vec![ShellCommand::None]
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.home_index = (self.home_index + HOME_ITEMS - 1) % HOME_ITEMS;
                vec![ShellCommand::None]
            }
            KeyCode::Enter => {
                let route = match self.home_index {
                    0 => Route::Library,
                    1 => Route::Installed,
                    2 => Route::Settings,
                    _ => Route::Library,
                };
                vec![ShellCommand::OpenRoute(route)]
            }
            _ => vec![ShellCommand::None],
        }
    }

    fn handle_library_key(&mut self, key: KeyEvent) -> Vec<ShellCommand> {
        let len = self.games.len().max(1);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.list_index = (self.list_index + 1) % len;
                vec![ShellCommand::None]
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.list_index = (self.list_index + len - 1) % len;
                vec![ShellCommand::None]
            }
            KeyCode::Esc => vec![ShellCommand::OpenRoute(Route::Home)],
            KeyCode::Enter => {
                if let Some(game) = self.games.get(self.list_index) {
                    vec![ShellCommand::OpenRoute(Route::GameDetail {
                        id: game.id.clone(),
                    })]
                } else {
                    vec![ShellCommand::None]
                }
            }
            _ => vec![ShellCommand::None],
        }
    }

    fn handle_settings_key(&mut self, key: KeyEvent) -> Vec<ShellCommand> {
        match key.code {
            KeyCode::Esc => vec![ShellCommand::OpenRoute(Route::Home)],
            KeyCode::Enter | KeyCode::Char(' ') => vec![ShellCommand::CyclePerformance],
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                self.settings_index = 0;
                vec![ShellCommand::None]
            }
            _ => vec![ShellCommand::None],
        }
    }

    fn handle_detail_key(&mut self, key: KeyEvent, id: String) -> Vec<ShellCommand> {
        match key.code {
            KeyCode::Esc => vec![ShellCommand::OpenRoute(Route::Library)],
            KeyCode::Enter => vec![ShellCommand::StartGame(id)],
            _ => vec![ShellCommand::None],
        }
    }

    fn handle_runner_key(&mut self, key: KeyEvent, has_running_game: bool) -> Vec<ShellCommand> {
        if !has_running_game {
            return vec![ShellCommand::OpenRoute(Route::Library)];
        }

        match key.code {
            KeyCode::Esc => vec![
                ShellCommand::StopGame,
                ShellCommand::OpenRoute(Route::Library),
            ],
            KeyCode::Char('p') | KeyCode::Char('P') => vec![ShellCommand::TogglePause],
            KeyCode::Char('r') | KeyCode::Char('R') => vec![ShellCommand::RestartGame],
            KeyCode::Char('f') | KeyCode::Char('F') => vec![ShellCommand::ToggleFullscreen],
            _ => vec![ShellCommand::None],
        }
    }

    pub fn selected_game(&self) -> Option<&GameItem> {
        self.games.get(self.list_index)
    }
}

pub fn render(frame: &mut ratatui::Frame<'_>, state: &ShellState, context: &RenderContext) {
    frame.render_widget(Clear, frame.area());
    frame.render_widget(Block::default().style(app_base_style()), frame.area());

    if matches!(state.route, Route::Runner) && context.runner_fullscreen {
        render_runner_fullscreen(frame, context, state);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(frame.area());

    let body = chunks[0];
    let status = chunks[1];

    match &state.route {
        Route::Home => render_home(frame, body, state),
        Route::Library => render_library(frame, body, state, "Library"),
        Route::Installed => render_library(frame, body, state, "Installed"),
        Route::Settings => render_settings(frame, body, state),
        Route::GameDetail { id } => render_detail(frame, body, state, id),
        Route::Runner => render_runner(frame, body, context),
    }

    render_status(frame, status, state, context);
    render_overlay(frame, state);
}

fn render_home(frame: &mut ratatui::Frame<'_>, area: Rect, state: &ShellState) {
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let items = ["Library", "Installed", "Settings", "Featured"];
    let list_items = items
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let style = if idx == state.home_index {
                selected_style()
            } else {
                Style::default().fg(FORGE.fg)
            };
            ListItem::new(Line::from(Span::styled(*item, style)))
        })
        .collect::<Vec<_>>();

    let left = List::new(list_items)
        .block(
            Block::default()
                .title("Home")
                .borders(Borders::ALL)
                .style(panel_style()),
        )
        .highlight_style(selected_style());

    let right = Paragraph::new(vec![
        Line::from(Span::styled("Dark Forest", title_style())),
        Line::from(""),
        Line::from("Terminal-native arcade console."),
        Line::from(Span::styled(
            "Mission: GUI-like UX in the terminal.",
            muted_style(),
        )),
        Line::from(""),
        Line::from("Use Enter to navigate."),
    ])
    .block(
        Block::default()
            .title("Overview")
            .borders(Borders::ALL)
            .style(panel_style()),
    )
    .wrap(Wrap { trim: true });

    frame.render_widget(left, main[0]);
    frame.render_widget(right, main[1]);
}

fn render_library(frame: &mut ratatui::Frame<'_>, area: Rect, state: &ShellState, title: &str) {
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let list_items = state
        .games
        .iter()
        .enumerate()
        .map(|(idx, game)| {
            let style = if idx == state.list_index {
                selected_style()
            } else {
                Style::default().fg(FORGE.fg)
            };
            ListItem::new(Line::from(Span::styled(game.name.clone(), style)))
        })
        .collect::<Vec<_>>();

    let left = List::new(list_items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .style(panel_style()),
        )
        .highlight_style(selected_style());
    frame.render_widget(left, main[0]);

    let detail = state.selected_game().map_or_else(
        || "No game selected".to_string(),
        |g| format!("{}\n\n{}\n\nPress Enter for detail.", g.name, g.description),
    );
    frame.render_widget(
        Paragraph::new(detail)
            .block(
                Block::default()
                    .title("Details")
                    .borders(Borders::ALL)
                    .style(panel_style()),
            )
            .wrap(Wrap { trim: true }),
        main[1],
    );
}

fn render_settings(frame: &mut ratatui::Frame<'_>, area: Rect, state: &ShellState) {
    let content = vec![
        Line::from(Span::styled("Settings", title_style())),
        Line::from(""),
        Line::from(format!("Performance: {}", state.performance_mode)),
        Line::from(Span::styled(
            "Press Enter to cycle Auto/60/30",
            muted_style(),
        )),
    ];

    frame.render_widget(
        Paragraph::new(content)
            .block(
                Block::default()
                    .title("Settings")
                    .borders(Borders::ALL)
                    .style(panel_style()),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_detail(frame: &mut ratatui::Frame<'_>, area: Rect, state: &ShellState, id: &str) {
    let selected = state.games.iter().find(|g| g.id == *id);
    let text = selected.map_or_else(
        || format!("Unknown game: {id}"),
        |game| {
            format!(
                "{}\n\n{}\n\nControls:\n- Move: arrows or WASD\n- Pause: P\n- Restart: R\n- Exit: Esc\n\nPress Enter to start. Esc to go back.",
                game.name, game.description
            )
        },
    );

    frame.render_widget(
        Paragraph::new(text)
            .block(
                Block::default()
                    .title("Game Detail")
                    .borders(Borders::ALL)
                    .style(panel_style()),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_runner(frame: &mut ratatui::Frame<'_>, area: Rect, context: &RenderContext) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(area);

    let game_area = chunks[0];
    let hud_area = chunks[1];

    if let Some(game_frame) = &context.runner_frame {
        draw_runtime_frame(frame, game_area, game_frame);
    } else {
        frame.render_widget(
            Paragraph::new("No active game").block(
                Block::default()
                    .title("Runner")
                    .borders(Borders::ALL)
                    .style(panel_style()),
            ),
            game_area,
        );
    }

    let status = format!(
        "Game: {} | {} | {}",
        context
            .current_game
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        if context.runner_paused {
            "paused"
        } else {
            "running"
        },
        context.perf_summary,
    );

    frame.render_widget(
        Paragraph::new(status).block(
            Block::default()
                .title("Runner HUD")
                .borders(Borders::ALL)
                .style(panel_style()),
        ),
        hud_area,
    );
}

fn render_runner_fullscreen(
    frame: &mut ratatui::Frame<'_>,
    context: &RenderContext,
    state: &ShellState,
) {
    if let Some(game_frame) = &context.runner_frame {
        draw_runtime_frame(frame, frame.area(), game_frame);
    }

    let hud = Paragraph::new(format!(
        "{} | {} | {}",
        context
            .current_game
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        if context.runner_paused {
            "paused"
        } else {
            "running"
        },
        context.perf_summary
    ))
    .style(
        Style::default()
            .fg(FORGE.accent_bright)
            .add_modifier(Modifier::BOLD),
    );

    let rect = Rect {
        x: 1,
        y: frame.area().height.saturating_sub(2),
        width: frame.area().width.saturating_sub(2),
        height: 1,
    };
    frame.render_widget(hud, rect);

    render_overlay(frame, state);
}

fn render_status(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    state: &ShellState,
    context: &RenderContext,
) {
    let status = vec![
        Line::from(vec![
            Span::styled("/ Search", muted_style()),
            Span::raw(" | "),
            Span::styled("Ctrl+K Palette", muted_style()),
            Span::raw(" | "),
            Span::styled("? Help", muted_style()),
            Span::raw(" | "),
            Span::styled("Ctrl+Q Quit", muted_style()),
        ]),
        Line::from(format!(
            "Route: {:?} | {}",
            state.route, context.perf_summary
        )),
    ];

    frame.render_widget(
        Paragraph::new(status)
            .block(
                Block::default()
                    .title("Status")
                    .borders(Borders::ALL)
                    .style(panel_style()),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_overlay(frame: &mut ratatui::Frame<'_>, state: &ShellState) {
    let Some(overlay) = state.overlay else {
        return;
    };

    let popup = centered_rect(70, 60, frame.area());
    frame.render_widget(Clear, popup);

    let (title, body) = match overlay {
        Overlay::CommandPalette => (
            "Command Palette",
            "Commands are coming online. Esc to close.".to_string(),
        ),
        Overlay::Search => ("Search", "Search is contextual. Esc to close.".to_string()),
        Overlay::Help => (
            "Help",
            "Global: ↑/↓ or j/k, Enter, Esc, /, Ctrl+K, ?, Ctrl+Q\nRunner: P, R, F, Esc"
                .to_string(),
        ),
        Overlay::Notifications => (
            "Notifications",
            if state.notifications.is_empty() {
                "No notifications".to_string()
            } else {
                state.notifications.join("\n")
            },
        ),
        Overlay::Progress => (
            "Progress",
            if state.progress.is_empty() {
                "No active tasks".to_string()
            } else {
                state.progress.join("\n")
            },
        ),
        Overlay::ErrorDetail => (
            "Error",
            state
                .last_error
                .clone()
                .unwrap_or_else(|| "No error details available".to_string()),
        ),
    };

    frame.render_widget(
        Paragraph::new(body)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .style(panel_style()),
            )
            .wrap(Wrap { trim: true }),
        popup,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1]);

    horizontal[1]
}

fn draw_runtime_frame(frame: &mut ratatui::Frame<'_>, area: Rect, game_frame: &Frame) {
    let mut lines = Vec::new();

    let max_rows = usize::min(
        usize::from(area.height.saturating_sub(2)),
        usize::from(game_frame.height),
    );
    let max_cols = usize::min(
        usize::from(area.width.saturating_sub(2)),
        usize::from(game_frame.width),
    );

    for y in 0..max_rows {
        let mut spans = Vec::new();
        for x in 0..max_cols {
            if let Some(cell) =
                game_frame.get(u16::try_from(x).unwrap_or(0), u16::try_from(y).unwrap_or(0))
            {
                spans.push(Span::styled(
                    cell.glyph.to_string(),
                    Style::default().fg(cell.fg).bg(cell.bg),
                ));
            }
        }
        lines.push(Line::from(spans));
    }

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Runner")
                .borders(Borders::ALL)
                .style(panel_style()),
        )
        .wrap(Wrap { trim: false });

    frame.render_widget(widget, area);
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use pretty_assertions::assert_eq;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::{GameItem, Overlay, RenderContext, Route, ShellCommand, ShellState, render};

    #[test]
    fn route_transitions_from_home_to_library() {
        let mut state = ShellState::new(sample_games());
        let commands =
            state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()), false);
        assert!(matches!(
            commands[0],
            ShellCommand::OpenRoute(Route::Library)
        ));
    }

    #[test]
    fn overlay_opens_with_ctrl_k() {
        let mut state = ShellState::new(sample_games());
        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            false,
        );
        assert!(matches!(
            commands[0],
            ShellCommand::SetOverlay(Some(Overlay::CommandPalette))
        ));
    }

    #[test]
    fn runner_shortcuts_emit_expected_commands() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Runner;

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::empty()),
            true,
        );
        assert!(matches!(commands[0], ShellCommand::TogglePause));
    }

    #[test]
    fn renders_shell_at_common_sizes() -> std::io::Result<()> {
        for (w, h) in [(80, 24), (120, 40), (160, 48)] {
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend)?;
            let state = ShellState::new(sample_games());
            let context = RenderContext::default();

            terminal.draw(|frame| {
                render(frame, &state, &context);
            })?;

            let buffer = terminal.backend().buffer().clone();
            assert_buffer_contains(&buffer, "Home");
            assert_buffer_contains(&buffer, "Status");
        }
        Ok(())
    }

    fn assert_buffer_contains(buffer: &Buffer, needle: &str) {
        let mut content = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = &buffer[(x, y)];
                content.push(cell.symbol().chars().next().unwrap_or(' '));
            }
            content.push('\n');
        }

        assert!(content.contains(needle), "buffer did not contain {needle}");
    }

    fn sample_games() -> Vec<GameItem> {
        vec![
            GameItem {
                id: "snake-plus".to_string(),
                name: "Snake+".to_string(),
                description: "Arcade loop".to_string(),
            },
            GameItem {
                id: "tetris-like".to_string(),
                name: "Tetris-like".to_string(),
                description: "Timing game".to_string(),
            },
        ]
    }

    #[test]
    fn selected_game_tracks_list_index() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Library;
        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()), false);
        let selected_id = state.selected_game().map(|g| g.id.as_str());
        assert_eq!(selected_id, Some("tetris-like"));
    }
}
