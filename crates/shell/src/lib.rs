use std::collections::BTreeMap;

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
    RunnerPauseMenu,
    RunnerQuitConfirm,
    RunnerRestartConfirm,
}

#[derive(Debug, Clone, Default)]
pub struct GameStatsSummary {
    pub play_count: u64,
    pub best_score: Option<i64>,
    pub last_played_at: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct InstalledSummary {
    pub current_version: String,
    pub source: String,
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
    pub continue_game_id: Option<String>,
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
    pub game_stats: BTreeMap<String, GameStatsSummary>,
    pub installed: BTreeMap<String, InstalledSummary>,
}

impl Default for RenderContext {
    fn default() -> Self {
        Self {
            current_game: None,
            runner_frame: None,
            runner_paused: false,
            runner_fullscreen: false,
            perf_summary: "fps:auto".to_string(),
            game_stats: BTreeMap::new(),
            installed: BTreeMap::new(),
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
            continue_game_id: None,
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

    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        has_running_game: bool,
        runner_paused: bool,
    ) -> Vec<ShellCommand> {
        if let Some(active_overlay) = self.overlay {
            return self.handle_overlay_key(key, active_overlay, has_running_game, runner_paused);
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
            _ => self.handle_route_key(key, has_running_game, runner_paused),
        }
    }

    fn handle_overlay_key(
        &mut self,
        key: KeyEvent,
        overlay: Overlay,
        has_running_game: bool,
        runner_paused: bool,
    ) -> Vec<ShellCommand> {
        match overlay {
            Overlay::RunnerPauseMenu => {
                if !has_running_game {
                    return vec![ShellCommand::SetOverlay(None)];
                }

                match key.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('p') | KeyCode::Char('P') => {
                        vec![ShellCommand::TogglePause, ShellCommand::SetOverlay(None)]
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        vec![ShellCommand::SetOverlay(Some(
                            Overlay::RunnerRestartConfirm,
                        ))]
                    }
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        vec![ShellCommand::SetOverlay(Some(Overlay::RunnerQuitConfirm))]
                    }
                    _ => vec![ShellCommand::None],
                }
            }
            Overlay::RunnerQuitConfirm => {
                if !has_running_game {
                    return vec![ShellCommand::SetOverlay(None)];
                }

                match key.code {
                    KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => vec![
                        ShellCommand::StopGame,
                        ShellCommand::OpenRoute(Route::Library),
                        ShellCommand::SetOverlay(None),
                    ],
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                        if runner_paused {
                            vec![ShellCommand::SetOverlay(Some(Overlay::RunnerPauseMenu))]
                        } else {
                            vec![ShellCommand::SetOverlay(None)]
                        }
                    }
                    _ => vec![ShellCommand::None],
                }
            }
            Overlay::RunnerRestartConfirm => {
                if !has_running_game {
                    return vec![ShellCommand::SetOverlay(None)];
                }

                match key.code {
                    KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                        if runner_paused {
                            vec![
                                ShellCommand::RestartGame,
                                ShellCommand::SetOverlay(Some(Overlay::RunnerPauseMenu)),
                            ]
                        } else {
                            vec![ShellCommand::RestartGame, ShellCommand::SetOverlay(None)]
                        }
                    }
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                        if runner_paused {
                            vec![ShellCommand::SetOverlay(Some(Overlay::RunnerPauseMenu))]
                        } else {
                            vec![ShellCommand::SetOverlay(None)]
                        }
                    }
                    _ => vec![ShellCommand::None],
                }
            }
            _ => match key.code {
                KeyCode::Esc => vec![ShellCommand::SetOverlay(None)],
                KeyCode::Char('n') if overlay == Overlay::Help => {
                    vec![ShellCommand::SetOverlay(Some(Overlay::Notifications))]
                }
                _ => vec![ShellCommand::None],
            },
        }
    }

    fn handle_route_key(
        &mut self,
        key: KeyEvent,
        has_running_game: bool,
        runner_paused: bool,
    ) -> Vec<ShellCommand> {
        match self.route.clone() {
            Route::Home => self.handle_home_key(key),
            Route::Library | Route::Installed => self.handle_library_key(key),
            Route::Settings => self.handle_settings_key(key),
            Route::GameDetail { id } => self.handle_detail_key(key, id),
            Route::Runner => self.handle_runner_key(key, has_running_game, runner_paused),
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
                    0 => self
                        .continue_game_id
                        .clone()
                        .map_or(Route::Library, |id| Route::GameDetail { id }),
                    1 => Route::Library,
                    2 => Route::Installed,
                    _ => Route::Installed,
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

    fn handle_runner_key(
        &mut self,
        key: KeyEvent,
        has_running_game: bool,
        runner_paused: bool,
    ) -> Vec<ShellCommand> {
        if !has_running_game {
            return vec![ShellCommand::OpenRoute(Route::Library)];
        }

        match key.code {
            KeyCode::Esc => vec![ShellCommand::SetOverlay(Some(Overlay::RunnerQuitConfirm))],
            KeyCode::Char('p') | KeyCode::Char('P') => {
                if runner_paused {
                    vec![ShellCommand::TogglePause, ShellCommand::SetOverlay(None)]
                } else {
                    vec![
                        ShellCommand::TogglePause,
                        ShellCommand::SetOverlay(Some(Overlay::RunnerPauseMenu)),
                    ]
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                vec![ShellCommand::SetOverlay(Some(
                    Overlay::RunnerRestartConfirm,
                ))]
            }
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
        Route::Home => render_home(frame, body, state, context),
        Route::Library => render_library(frame, body, state, "Library"),
        Route::Installed => render_installed(frame, body, state, context),
        Route::Settings => render_settings(frame, body, state),
        Route::GameDetail { id } => render_detail(frame, body, state, context, id),
        Route::Runner => render_runner(frame, body, context),
    }

    render_status(frame, status, state, context);
    render_overlay(frame, state);
}

fn render_home(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    state: &ShellState,
    context: &RenderContext,
) {
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let items = ["Continue", "Featured", "Recently Played", "Updates"];
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

    let right_lines = home_detail_lines(state, context);
    let right = Paragraph::new(right_lines)
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

fn home_detail_lines(state: &ShellState, context: &RenderContext) -> Vec<Line<'static>> {
    match state.home_index {
        0 => {
            if let Some(id) = &state.continue_game_id
                && let Some(game) = state.games.iter().find(|item| &item.id == id)
            {
                let stats = context.game_stats.get(id).cloned().unwrap_or_default();
                let best = stats
                    .best_score
                    .map_or_else(|| "n/a".to_string(), |value| value.to_string());
                return vec![
                    Line::from(Span::styled("Continue", title_style())),
                    Line::from(""),
                    Line::from(format!("Next up: {}", game.name)),
                    Line::from(game.description.clone()),
                    Line::from(""),
                    Line::from(format!("Best score: {best}")),
                    Line::from(format!("Total plays: {}", stats.play_count)),
                    Line::from(Span::styled(
                        "Press Enter to resume from detail.",
                        muted_style(),
                    )),
                ];
            }

            vec![
                Line::from(Span::styled("Continue", title_style())),
                Line::from(""),
                Line::from("No recent game yet."),
                Line::from(Span::styled(
                    "Start a game from Library to populate this section.",
                    muted_style(),
                )),
            ]
        }
        1 => {
            let mut lines = vec![
                Line::from(Span::styled("Featured", title_style())),
                Line::from(""),
            ];
            for game in state.games.iter().take(3) {
                lines.push(Line::from(format!("- {}", game.name)));
            }
            if state.games.is_empty() {
                lines.push(Line::from("No featured titles available."));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Press Enter to open Library.",
                muted_style(),
            )));
            lines
        }
        2 => {
            let mut entries = context
                .game_stats
                .iter()
                .map(|(id, stats)| (id.clone(), stats.last_played_at.clone(), stats.play_count))
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| b.1.cmp(&a.1));

            let mut lines = vec![
                Line::from(Span::styled("Recently Played", title_style())),
                Line::from(""),
            ];

            for (id, _last_played, play_count) in entries.into_iter().take(5) {
                if play_count > 0 {
                    let name = state
                        .games
                        .iter()
                        .find(|game| game.id == id)
                        .map_or(id.clone(), |game| game.name.clone());
                    lines.push(Line::from(format!("- {name} ({play_count} plays)")));
                }
            }

            if lines.len() <= 2 {
                lines.push(Line::from("No play history yet."));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Press Enter to open Installed.",
                muted_style(),
            )));
            lines
        }
        _ => vec![
            Line::from(Span::styled("Updates", title_style())),
            Line::from(""),
            Line::from("All installed content is up to date."),
            Line::from(Span::styled(
                "Marketplace providers arrive in a later milestone.",
                muted_style(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press Enter to open Installed.",
                muted_style(),
            )),
        ],
    }
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

fn render_installed(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    state: &ShellState,
    context: &RenderContext,
) {
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let installed_ids = state
        .games
        .iter()
        .filter(|game| context.installed.contains_key(&game.id))
        .map(|game| game.id.clone())
        .collect::<Vec<_>>();

    let list_items = installed_ids
        .iter()
        .enumerate()
        .map(|(idx, game_id)| {
            let name = state
                .games
                .iter()
                .find(|game| game.id == *game_id)
                .map_or_else(|| game_id.clone(), |game| game.name.clone());

            let style = if idx == state.list_index {
                selected_style()
            } else {
                Style::default().fg(FORGE.fg)
            };
            ListItem::new(Line::from(Span::styled(name, style)))
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        List::new(list_items)
            .block(
                Block::default()
                    .title("Installed")
                    .borders(Borders::ALL)
                    .style(panel_style()),
            )
            .highlight_style(selected_style()),
        main[0],
    );

    let selected_id = installed_ids.get(state.list_index);
    let detail = selected_id.map_or_else(
        || "No installed game selected".to_string(),
        |id| {
            let game = state.games.iter().find(|item| item.id == *id);
            let installed = context.installed.get(id).cloned().unwrap_or_default();
            format!(
                "{}\n\n{}\n\nVersion: {}\nSource: {}\n\nActions:\n- Verify (planned)\n- Rollback (planned)\n- Update (planned)\n\nPress Enter for game detail.",
                game.map_or_else(|| id.clone(), |item| item.name.clone()),
                game.map_or_else(|| "No metadata available".to_string(), |item| item.description.clone()),
                if installed.current_version.is_empty() {
                    "unknown".to_string()
                } else {
                    installed.current_version
                },
                if installed.source.is_empty() {
                    "unknown".to_string()
                } else {
                    installed.source
                },
            )
        },
    );

    frame.render_widget(
        Paragraph::new(detail)
            .block(
                Block::default()
                    .title("Installed Detail")
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

fn render_detail(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    state: &ShellState,
    context: &RenderContext,
    id: &str,
) {
    let selected = state.games.iter().find(|g| g.id == *id);
    let stats = context.game_stats.get(id).cloned().unwrap_or_default();
    let best_score = stats
        .best_score
        .map_or_else(|| "n/a".to_string(), |value| value.to_string());
    let last_played = stats.last_played_at.unwrap_or_else(|| "never".to_string());
    let installed = context.installed.get(id).cloned().unwrap_or_default();
    let version = if installed.current_version.is_empty() {
        "unknown".to_string()
    } else {
        installed.current_version
    };

    let text = selected.map_or_else(
        || format!("Unknown game: {id}"),
        |game| {
            format!(
                "{}\n\n{}\n\nVersion: {}\n\nControls:\n- Move: arrows or WASD\n- Pause menu: P\n- Restart confirm: R\n- Quit confirm: Esc\n\nStats:\n- Plays: {}\n- Best score: {}\n- Last played: {}\n\nPress Enter to start. Esc to go back.",
                game.name,
                game.description,
                version,
                stats.play_count,
                best_score,
                last_played
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
        Overlay::RunnerPauseMenu => (
            "Paused",
            "Game paused.\n\nPress P or Enter to resume.\nPress R to restart (confirm).\nPress Q or Esc to exit (confirm).".to_string(),
        ),
        Overlay::RunnerQuitConfirm => (
            "Quit Game?",
            "Press Y or Enter to quit.\nPress N or Esc to cancel.".to_string(),
        ),
        Overlay::RunnerRestartConfirm => (
            "Restart Game?",
            "Press Y or Enter to restart.\nPress N or Esc to cancel.".to_string(),
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

    use super::{
        GameItem, GameStatsSummary, InstalledSummary, Overlay, RenderContext, Route, ShellCommand,
        ShellState, render,
    };

    #[test]
    fn route_transitions_from_home_to_library() {
        let mut state = ShellState::new(sample_games());
        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            false,
            false,
        );
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
            false,
        );
        assert_eq!(commands.len(), 2);
        assert!(matches!(commands[0], ShellCommand::TogglePause));
        assert!(matches!(
            commands[1],
            ShellCommand::SetOverlay(Some(Overlay::RunnerPauseMenu))
        ));
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
        state.handle_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
            false,
            false,
        );
        let selected_id = state.selected_game().map(|g| g.id.as_str());
        assert_eq!(selected_id, Some("tetris-like"));
    }

    #[test]
    fn continue_home_entry_opens_game_detail() {
        let mut state = ShellState::new(sample_games());
        state.continue_game_id = Some("snake-plus".to_string());
        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            false,
            false,
        );

        assert_eq!(commands.len(), 1);
        assert!(matches!(
            commands[0],
            ShellCommand::OpenRoute(Route::GameDetail { .. })
        ));
    }

    #[test]
    fn runner_esc_requests_quit_confirmation() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Runner;
        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
            true,
            false,
        );

        assert_eq!(commands.len(), 1);
        assert!(matches!(
            commands[0],
            ShellCommand::SetOverlay(Some(Overlay::RunnerQuitConfirm))
        ));
    }

    #[test]
    fn quit_confirmation_accepts_y() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Runner;
        state.overlay = Some(Overlay::RunnerQuitConfirm);
        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('y'), KeyModifiers::empty()),
            true,
            true,
        );

        assert_eq!(commands.len(), 3);
        assert!(matches!(commands[0], ShellCommand::StopGame));
        assert!(matches!(
            commands[1],
            ShellCommand::OpenRoute(Route::Library)
        ));
        assert!(matches!(commands[2], ShellCommand::SetOverlay(None)));
    }

    #[test]
    fn detail_screen_renders_stats() -> std::io::Result<()> {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend)?;
        let mut state = ShellState::new(sample_games());
        state.route = Route::GameDetail {
            id: "snake-plus".to_string(),
        };

        let mut context = RenderContext::default();
        context.game_stats.insert(
            "snake-plus".to_string(),
            GameStatsSummary {
                play_count: 12,
                best_score: Some(420),
                last_played_at: Some("2026-02-09T00:00:00Z".to_string()),
            },
        );

        terminal.draw(|frame| {
            render(frame, &state, &context);
        })?;

        let buffer = terminal.backend().buffer().clone();
        assert_buffer_contains(&buffer, "Best score");
        assert_buffer_contains(&buffer, "420");
        Ok(())
    }

    #[test]
    fn installed_route_renders_version_and_actions() -> std::io::Result<()> {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend)?;
        let mut state = ShellState::new(sample_games());
        state.route = Route::Installed;
        state.list_index = 0;

        let mut context = RenderContext::default();
        context.installed.insert(
            "snake-plus".to_string(),
            InstalledSummary {
                current_version: "0.1.0".to_string(),
                source: "builtin://dark-forest".to_string(),
            },
        );

        terminal.draw(|frame| {
            render(frame, &state, &context);
        })?;

        let buffer = terminal.backend().buffer().clone();
        assert_buffer_contains(&buffer, "Version:");
        assert_buffer_contains(&buffer, "Verify");
        Ok(())
    }
}
