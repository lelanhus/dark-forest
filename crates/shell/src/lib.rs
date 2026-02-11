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
    PermissionPrompt,
    RunnerPauseMenu,
    RunnerQuitConfirm,
    RunnerRestartConfirm,
    RunnerLeaveConfirm,
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PermissionAuditEntry {
    pub game_id: String,
    pub capability: String,
    pub decision: String,
    pub remembered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPromptState {
    pub game_id: String,
    pub capability: String,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub controls_summary: Vec<String>,
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
    pub command_palette_index: usize,
    pub search_query: String,
    pub installed_game_ids: Vec<String>,
    pub permission_audit_entries: Vec<PermissionAuditEntry>,
    pub permission_prompt: Option<PermissionPromptState>,
    pub runner_leave_target: Option<Route>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionPromptAction {
    AllowOnce,
    AllowAlways,
    DenyOnce,
    DenyAlways,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellCommand {
    Quit,
    OpenRoute(Route),
    StartGame(String),
    StopGame,
    RestartGame,
    ToggleFullscreen,
    TogglePause,
    InstallSelected(String),
    UpdateInstalled(String),
    RollbackInstalled(String),
    VerifyInstalled(String),
    RemoveInstalled(String),
    RevokePermission {
        game_id: String,
        capability: Option<String>,
    },
    ResolvePermissionPrompt(PermissionPromptAction),
    SetOverlay(Option<Overlay>),
    CyclePerformance,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaletteAction {
    OpenHome,
    OpenLibrary,
    OpenInstalled,
    OpenSettings,
    StartSelectedGame,
    TogglePerformance,
    OpenDiagnostics,
    OpenHelp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PaletteCommand {
    label: &'static str,
    action: PaletteAction,
}

const PALETTE_COMMANDS: [PaletteCommand; 8] = [
    PaletteCommand {
        label: "Open Home",
        action: PaletteAction::OpenHome,
    },
    PaletteCommand {
        label: "Open Library",
        action: PaletteAction::OpenLibrary,
    },
    PaletteCommand {
        label: "Open Installed",
        action: PaletteAction::OpenInstalled,
    },
    PaletteCommand {
        label: "Open Settings",
        action: PaletteAction::OpenSettings,
    },
    PaletteCommand {
        label: "Start Selected Game",
        action: PaletteAction::StartSelectedGame,
    },
    PaletteCommand {
        label: "Toggle Performance Mode",
        action: PaletteAction::TogglePerformance,
    },
    PaletteCommand {
        label: "Open Diagnostics",
        action: PaletteAction::OpenDiagnostics,
    },
    PaletteCommand {
        label: "Open Help",
        action: PaletteAction::OpenHelp,
    },
];

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
            command_palette_index: 0,
            search_query: String::new(),
            installed_game_ids: Vec::new(),
            permission_audit_entries: Vec::new(),
            permission_prompt: None,
            runner_leave_target: None,
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

    pub fn set_installed_game_ids(&mut self, ids: Vec<String>) {
        self.installed_game_ids = ids;
        self.normalize_list_index();
    }

    pub fn set_permission_audit_entries(&mut self, entries: Vec<PermissionAuditEntry>) {
        self.permission_audit_entries = entries;
        let max_index = self.permission_audit_entries.len();
        if self.settings_index > max_index {
            self.settings_index = max_index;
        }
    }

    pub fn set_permission_prompt(&mut self, prompt: Option<PermissionPromptState>) {
        self.permission_prompt = prompt;
    }

    fn is_installed_game_id(&self, id: &str) -> bool {
        self.installed_game_ids
            .iter()
            .any(|installed| installed == id)
    }

    fn normalize_list_index(&mut self) {
        let visible_len = self.visible_game_indices_for_route(&self.route).len();
        if visible_len == 0 {
            self.list_index = 0;
            return;
        }

        if self.list_index >= visible_len {
            self.list_index = visible_len - 1;
        }
    }

    fn query(&self) -> String {
        self.search_query.trim().to_lowercase()
    }

    fn game_matches_query(&self, game: &GameItem) -> bool {
        let query = self.query();
        if query.is_empty() {
            return true;
        }

        game.id.to_lowercase().contains(&query)
            || game.name.to_lowercase().contains(&query)
            || game.description.to_lowercase().contains(&query)
            || game
                .tags
                .iter()
                .any(|tag| tag.to_lowercase().contains(&query))
    }

    fn filtered_game_indices(&self) -> Vec<usize> {
        self.games
            .iter()
            .enumerate()
            .filter_map(|(idx, game)| self.game_matches_query(game).then_some(idx))
            .collect()
    }

    fn visible_game_indices_for_route(&self, route: &Route) -> Vec<usize> {
        let filtered = self.filtered_game_indices();
        if matches!(route, Route::Installed) {
            return filtered
                .into_iter()
                .filter(|idx| {
                    self.installed_game_ids
                        .iter()
                        .any(|id| id == &self.games[*idx].id)
                })
                .collect();
        }

        filtered
    }

    fn selected_game_for_route(&self, route: &Route) -> Option<&GameItem> {
        let visible = self.visible_game_indices_for_route(route);
        if visible.is_empty() {
            return None;
        }

        let selected = self.list_index.min(visible.len() - 1);
        self.games.get(visible[selected])
    }

    fn selected_start_target(&self) -> Option<String> {
        match &self.route {
            Route::GameDetail { id } => Some(id.clone()),
            Route::Library => self
                .selected_game_for_route(&Route::Library)
                .map(|game| game.id.clone()),
            Route::Installed => self
                .selected_game_for_route(&Route::Installed)
                .map(|game| game.id.clone()),
            _ => self
                .continue_game_id
                .clone()
                .or_else(|| self.games.first().map(|game| game.id.clone())),
        }
    }

    fn route_palette_action(&mut self, route: Route, has_running_game: bool) -> Vec<ShellCommand> {
        if matches!(self.route, Route::Runner) && has_running_game {
            self.runner_leave_target = Some(route);
            return vec![ShellCommand::SetOverlay(Some(Overlay::RunnerLeaveConfirm))];
        }

        vec![
            ShellCommand::OpenRoute(route),
            ShellCommand::SetOverlay(None),
        ]
    }

    fn execute_palette_action(
        &mut self,
        action: PaletteAction,
        has_running_game: bool,
    ) -> Vec<ShellCommand> {
        match action {
            PaletteAction::OpenHome => self.route_palette_action(Route::Home, has_running_game),
            PaletteAction::OpenLibrary => {
                self.route_palette_action(Route::Library, has_running_game)
            }
            PaletteAction::OpenInstalled => {
                self.route_palette_action(Route::Installed, has_running_game)
            }
            PaletteAction::OpenSettings => {
                self.route_palette_action(Route::Settings, has_running_game)
            }
            PaletteAction::StartSelectedGame => {
                if let Some(id) = self.selected_start_target() {
                    vec![ShellCommand::StartGame(id), ShellCommand::SetOverlay(None)]
                } else {
                    vec![ShellCommand::SetOverlay(None)]
                }
            }
            PaletteAction::TogglePerformance => {
                vec![
                    ShellCommand::CyclePerformance,
                    ShellCommand::SetOverlay(None),
                ]
            }
            PaletteAction::OpenDiagnostics => {
                self.route_palette_action(Route::Settings, has_running_game)
            }
            PaletteAction::OpenHelp => {
                vec![ShellCommand::SetOverlay(Some(Overlay::Help))]
            }
        }
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
                    self.command_palette_index = 0;
                    return vec![ShellCommand::SetOverlay(Some(Overlay::CommandPalette))];
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Char('/') => {
                self.search_query.clear();
                self.list_index = 0;
                vec![ShellCommand::SetOverlay(Some(Overlay::Search))]
            }
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
            Overlay::CommandPalette => match key.code {
                KeyCode::Esc => vec![ShellCommand::SetOverlay(None)],
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = PALETTE_COMMANDS.len().max(1);
                    self.command_palette_index = (self.command_palette_index + 1) % len;
                    vec![ShellCommand::None]
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    let len = PALETTE_COMMANDS.len().max(1);
                    self.command_palette_index = (self.command_palette_index + len - 1) % len;
                    vec![ShellCommand::None]
                }
                KeyCode::Enter => {
                    let selected = self.command_palette_index.min(PALETTE_COMMANDS.len() - 1);
                    self.execute_palette_action(PALETTE_COMMANDS[selected].action, has_running_game)
                }
                _ => vec![ShellCommand::None],
            },
            Overlay::Search => match key.code {
                KeyCode::Esc => {
                    self.search_query.clear();
                    self.list_index = 0;
                    vec![ShellCommand::SetOverlay(None)]
                }
                KeyCode::Enter => vec![ShellCommand::SetOverlay(None)],
                KeyCode::Backspace => {
                    let _ = self.search_query.pop();
                    self.list_index = 0;
                    vec![ShellCommand::None]
                }
                KeyCode::Char(ch) => {
                    if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                        self.search_query.push(ch);
                        self.list_index = 0;
                    }
                    vec![ShellCommand::None]
                }
                _ => vec![ShellCommand::None],
            },
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
            Overlay::RunnerLeaveConfirm => {
                if !has_running_game {
                    self.runner_leave_target = None;
                    return vec![ShellCommand::SetOverlay(None)];
                }

                match key.code {
                    KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                        let target = self.runner_leave_target.take().unwrap_or(Route::Home);
                        vec![
                            ShellCommand::StopGame,
                            ShellCommand::OpenRoute(target),
                            ShellCommand::SetOverlay(None),
                        ]
                    }
                    KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.runner_leave_target = None;
                        if runner_paused {
                            vec![ShellCommand::SetOverlay(Some(Overlay::RunnerPauseMenu))]
                        } else {
                            vec![ShellCommand::SetOverlay(None)]
                        }
                    }
                    _ => vec![ShellCommand::None],
                }
            }
            Overlay::PermissionPrompt => match key.code {
                KeyCode::Char('1') | KeyCode::Char('a') | KeyCode::Char('A') => {
                    vec![ShellCommand::ResolvePermissionPrompt(
                        PermissionPromptAction::AllowOnce,
                    )]
                }
                KeyCode::Char('2') => vec![ShellCommand::ResolvePermissionPrompt(
                    PermissionPromptAction::AllowAlways,
                )],
                KeyCode::Char('3') | KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Esc => {
                    vec![ShellCommand::ResolvePermissionPrompt(
                        PermissionPromptAction::DenyOnce,
                    )]
                }
                KeyCode::Char('4') => vec![ShellCommand::ResolvePermissionPrompt(
                    PermissionPromptAction::DenyAlways,
                )],
                _ => vec![ShellCommand::None],
            },
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
            Route::Library => self.handle_collection_key(key, Route::Library),
            Route::Installed => self.handle_collection_key(key, Route::Installed),
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

    fn handle_collection_key(&mut self, key: KeyEvent, route: Route) -> Vec<ShellCommand> {
        self.normalize_list_index();
        let visible = self.visible_game_indices_for_route(&route);
        let len = visible.len().max(1);
        let selected_id = visible
            .get(self.list_index.min(visible.len().saturating_sub(1)))
            .and_then(|idx| self.games.get(*idx))
            .map(|game| game.id.clone());
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
                if let Some(game_idx) =
                    visible.get(self.list_index.min(visible.len().saturating_sub(1)))
                    && let Some(game) = self.games.get(*game_idx)
                {
                    vec![ShellCommand::OpenRoute(Route::GameDetail {
                        id: game.id.clone(),
                    })]
                } else {
                    vec![ShellCommand::None]
                }
            }
            KeyCode::Char('u') | KeyCode::Char('U') if matches!(route, Route::Installed) => {
                selected_id
                    .map(|id| vec![ShellCommand::UpdateInstalled(id)])
                    .unwrap_or_else(|| vec![ShellCommand::None])
            }
            KeyCode::Char('i') | KeyCode::Char('I') if matches!(route, Route::Library) => {
                if let Some(id) = selected_id {
                    if self.is_installed_game_id(&id) {
                        vec![ShellCommand::None]
                    } else {
                        vec![ShellCommand::InstallSelected(id)]
                    }
                } else {
                    vec![ShellCommand::None]
                }
            }
            KeyCode::Char('b') | KeyCode::Char('B') if matches!(route, Route::Installed) => {
                selected_id
                    .map(|id| vec![ShellCommand::RollbackInstalled(id)])
                    .unwrap_or_else(|| vec![ShellCommand::None])
            }
            KeyCode::Char('v') | KeyCode::Char('V') if matches!(route, Route::Installed) => {
                selected_id
                    .map(|id| vec![ShellCommand::VerifyInstalled(id)])
                    .unwrap_or_else(|| vec![ShellCommand::None])
            }
            KeyCode::Char('x') | KeyCode::Char('X') if matches!(route, Route::Installed) => {
                selected_id
                    .map(|id| vec![ShellCommand::RemoveInstalled(id)])
                    .unwrap_or_else(|| vec![ShellCommand::None])
            }
            _ => vec![ShellCommand::None],
        }
    }

    fn handle_settings_key(&mut self, key: KeyEvent) -> Vec<ShellCommand> {
        let total_items = 1 + self.permission_audit_entries.len();
        match key.code {
            KeyCode::Esc => vec![ShellCommand::OpenRoute(Route::Home)],
            KeyCode::Down | KeyCode::Char('j') => {
                self.settings_index = (self.settings_index + 1) % total_items.max(1);
                vec![ShellCommand::None]
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.settings_index =
                    (self.settings_index + total_items.max(1) - 1) % total_items.max(1);
                vec![ShellCommand::None]
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.settings_index == 0 {
                    vec![ShellCommand::CyclePerformance]
                } else if let Some(entry) =
                    self.permission_audit_entries.get(self.settings_index - 1)
                {
                    vec![ShellCommand::RevokePermission {
                        game_id: entry.game_id.clone(),
                        capability: Some(entry.capability.clone()),
                    }]
                } else {
                    vec![ShellCommand::None]
                }
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                if self.settings_index == 0 {
                    vec![ShellCommand::None]
                } else if let Some(entry) =
                    self.permission_audit_entries.get(self.settings_index - 1)
                {
                    vec![ShellCommand::RevokePermission {
                        game_id: entry.game_id.clone(),
                        capability: Some(entry.capability.clone()),
                    }]
                } else {
                    vec![ShellCommand::None]
                }
            }
            _ => vec![ShellCommand::None],
        }
    }

    fn handle_detail_key(&mut self, key: KeyEvent, id: String) -> Vec<ShellCommand> {
        match key.code {
            KeyCode::Esc => vec![ShellCommand::OpenRoute(Route::Library)],
            KeyCode::Enter => vec![ShellCommand::StartGame(id)],
            KeyCode::Char('x') | KeyCode::Char('X')
                if self
                    .installed_game_ids
                    .iter()
                    .any(|installed| installed == &id) =>
            {
                vec![ShellCommand::RemoveInstalled(id)]
            }
            KeyCode::Char('i') | KeyCode::Char('I') if !self.is_installed_game_id(&id) => {
                vec![ShellCommand::InstallSelected(id)]
            }
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
        self.selected_game_for_route(&self.route)
    }

    pub fn visible_games_for_route(&self, route: &Route) -> Vec<&GameItem> {
        self.visible_game_indices_for_route(route)
            .into_iter()
            .filter_map(|idx| self.games.get(idx))
            .collect()
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
        Route::Library => render_library(frame, body, state, Route::Library, "Library"),
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
                "Marketplace registries are configurable in Settings.",
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

fn render_library(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    state: &ShellState,
    route: Route,
    title: &str,
) {
    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    let visible_games = state.visible_games_for_route(&route);
    let selected = if visible_games.is_empty() {
        0
    } else {
        state.list_index.min(visible_games.len() - 1)
    };

    let list_items = visible_games
        .iter()
        .enumerate()
        .map(|(idx, game)| {
            let style = if idx == selected {
                selected_style()
            } else {
                Style::default().fg(FORGE.fg)
            };
            ListItem::new(Line::from(Span::styled(game.name.to_string(), style)))
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

    let detail = visible_games.get(selected).map_or_else(
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

    let installed_games = state
        .visible_games_for_route(&Route::Installed)
        .iter()
        .filter(|game| context.installed.contains_key(&game.id))
        .copied()
        .collect::<Vec<_>>();

    let selected = if installed_games.is_empty() {
        0
    } else {
        state.list_index.min(installed_games.len() - 1)
    };

    let list_items = installed_games
        .iter()
        .enumerate()
        .map(|(idx, game)| {
            let style = if idx == selected {
                selected_style()
            } else {
                Style::default().fg(FORGE.fg)
            };
            ListItem::new(Line::from(Span::styled(game.name.clone(), style)))
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

    let detail = installed_games.get(selected).map_or_else(
        || "No installed game selected".to_string(),
        |game| {
            let installed = context.installed.get(&game.id).cloned().unwrap_or_default();
            format!(
                "{}\n\n{}\n\nVersion: {}\nSource: {}\n\nActions:\n- [V] Verify\n- [B] Rollback\n- [U] Update\n- [X] Remove\n\nPress Enter for game detail.",
                game.name,
                game.description,
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
    let mut content = vec![
        Line::from(Span::styled("Settings", title_style())),
        Line::from(""),
    ];
    let perf_marker = if state.settings_index == 0 { ">" } else { " " };
    content.push(Line::from(format!(
        "{perf_marker} Performance: {} (Enter to cycle Auto/60/30)",
        state.performance_mode
    )));
    content.push(Line::from(""));
    content.push(Line::from(Span::styled(
        "Permissions Audit (select entry and press Enter/X to revoke):",
        muted_style(),
    )));

    if state.permission_audit_entries.is_empty() {
        content.push(Line::from("  no remembered grants"));
    } else {
        for (idx, entry) in state.permission_audit_entries.iter().enumerate() {
            let marker = if state.settings_index == idx + 1 {
                ">"
            } else {
                " "
            };
            let memory = if entry.remembered {
                "remembered"
            } else {
                "session"
            };
            content.push(Line::from(format!(
                "{marker} {} :: {} ({}, {memory})",
                entry.game_id, entry.capability, entry.decision
            )));
        }
    }

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
    let is_installed = context.installed.contains_key(id);
    let version = if installed.current_version.is_empty() {
        "unknown".to_string()
    } else {
        installed.current_version
    };

    let text = selected.map_or_else(
        || format!("Unknown game: {id}"),
        |game| {
            let controls = if game.controls_summary.is_empty() {
                vec![
                    "Move: arrows or WASD".to_string(),
                    "Pause menu: P".to_string(),
                    "Restart confirm: R".to_string(),
                    "Quit confirm: Esc".to_string(),
                ]
            } else {
                game.controls_summary.clone()
            };
            let controls_text = controls
                .into_iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{}\n\n{}\n\nVersion: {}\n\nControls:\n{}\n\nStats:\n- Plays: {}\n- Best score: {}\n- Last played: {}\n\nActions:\n- Enter: start game{}\n- Esc: back",
                game.name,
                game.description,
                version,
                controls_text,
                stats.play_count,
                best_score,
                last_played,
                if is_installed {
                    "\n- X: remove installed copy"
                } else {
                    "\n- [I] Install from registry"
                }
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
        Overlay::CommandPalette => ("Command Palette", command_palette_body(state)),
        Overlay::Search => ("Search", search_overlay_body(state)),
        Overlay::Help => (
            "Help",
            "Global: ↑/↓ or j/k, Enter, Esc, /, Ctrl+K, ?, Ctrl+Q\nRunner: P, R, F, Esc\nPermissions prompt: 1 Allow Once, 2 Allow Always, 3 Deny Once, 4 Deny Always"
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
        Overlay::PermissionPrompt => {
            let body = state.permission_prompt.clone().map_or_else(
                || "No active permission request.".to_string(),
                |prompt| {
                    format!(
                        "{}\n\nGame: {}\nCapability: {}\n\nChoose:\n1) Allow Once\n2) Allow Always\n3) Deny Once\n4) Deny Always",
                        prompt.prompt, prompt.game_id, prompt.capability
                    )
                },
            );
            ("Permission Request", body)
        }
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
        Overlay::RunnerLeaveConfirm => (
            "Leave Active Game?",
            "An active run is in progress.\n\nPress Y or Enter to stop and leave.\nPress N or Esc to stay in the game.".to_string(),
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

fn command_palette_body(state: &ShellState) -> String {
    let mut lines = vec!["Select a command:".to_string(), String::new()];

    let selected = state.command_palette_index.min(PALETTE_COMMANDS.len() - 1);
    for (idx, command) in PALETTE_COMMANDS.iter().enumerate() {
        let marker = if idx == selected { ">" } else { " " };
        lines.push(format!("{marker} {}", command.label));
    }

    lines.push(String::new());
    lines.push("Enter executes. Esc closes.".to_string());
    lines.join("\n")
}

fn search_overlay_body(state: &ShellState) -> String {
    let route = if matches!(state.route, Route::Installed) {
        Route::Installed
    } else {
        Route::Library
    };
    let matches = state.visible_game_indices_for_route(&route).len();
    let route_label = if matches!(route, Route::Installed) {
        "Installed"
    } else {
        "Library"
    };

    format!(
        "Route: {route_label}\nQuery: {}\nMatches: {matches}\n\nType to filter by id, name, description, or tags.\nBackspace deletes. Esc clears and closes.",
        if state.search_query.is_empty() {
            "<empty>"
        } else {
            &state.search_query
        }
    )
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
        GameItem, GameStatsSummary, InstalledSummary, Overlay, PermissionAuditEntry,
        PermissionPromptAction, PermissionPromptState, RenderContext, Route, ShellCommand,
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

    fn assert_buffer_not_contains(buffer: &Buffer, needle: &str) {
        let mut content = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = &buffer[(x, y)];
                content.push(cell.symbol().chars().next().unwrap_or(' '));
            }
            content.push('\n');
        }

        assert!(
            !content.contains(needle),
            "buffer unexpectedly contained {needle}"
        );
    }

    fn sample_games() -> Vec<GameItem> {
        vec![
            GameItem {
                id: "snake-plus".to_string(),
                name: "Snake+".to_string(),
                description: "Arcade loop".to_string(),
                tags: vec!["arcade".to_string(), "builtin".to_string()],
                controls_summary: vec![
                    "Move: arrows or WASD".to_string(),
                    "Avoid walls and yourself".to_string(),
                ],
            },
            GameItem {
                id: "tetris-like".to_string(),
                name: "Tetris-like".to_string(),
                description: "Timing game".to_string(),
                tags: vec![
                    "arcade".to_string(),
                    "puzzle".to_string(),
                    "builtin".to_string(),
                ],
                controls_summary: vec![
                    "Move: arrows or WASD".to_string(),
                    "Rotate: Z/X".to_string(),
                    "Hold: C".to_string(),
                    "Hard drop: Space".to_string(),
                ],
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
    fn detail_screen_renders_install_action_for_non_installed_game() -> std::io::Result<()> {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend)?;
        let mut state = ShellState::new(sample_games());
        state.route = Route::GameDetail {
            id: "snake-plus".to_string(),
        };

        let context = RenderContext::default();

        terminal.draw(|frame| {
            render(frame, &state, &context);
        })?;

        let buffer = terminal.backend().buffer().clone();
        assert_buffer_contains(&buffer, "[I] Install");
        Ok(())
    }

    #[test]
    fn installed_route_renders_version_and_actions() -> std::io::Result<()> {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend)?;
        let mut state = ShellState::new(sample_games());
        state.route = Route::Installed;
        state.list_index = 0;
        state.set_installed_game_ids(vec!["snake-plus".to_string()]);

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
        assert_buffer_contains(&buffer, "[V] Verify");
        assert_buffer_contains(&buffer, "[X] Remove");
        assert_buffer_not_contains(&buffer, "(planned)");
        Ok(())
    }

    #[test]
    fn home_updates_panel_no_longer_mentions_later_milestone() -> std::io::Result<()> {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend)?;
        let mut state = ShellState::new(sample_games());
        state.route = Route::Home;
        state.home_index = 3;

        let context = RenderContext::default();
        terminal.draw(|frame| {
            render(frame, &state, &context);
        })?;

        let buffer = terminal.backend().buffer().clone();
        assert_buffer_not_contains(
            &buffer,
            "Marketplace providers arrive in a later milestone.",
        );
        Ok(())
    }

    #[test]
    fn installed_action_keys_emit_operation_commands() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Installed;
        state.set_installed_game_ids(vec!["snake-plus".to_string()]);

        let update = state.handle_key(
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::empty()),
            false,
            false,
        );
        assert_eq!(
            update,
            vec![ShellCommand::UpdateInstalled("snake-plus".to_string())]
        );

        let rollback = state.handle_key(
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::empty()),
            false,
            false,
        );
        assert_eq!(
            rollback,
            vec![ShellCommand::RollbackInstalled("snake-plus".to_string())]
        );

        let verify = state.handle_key(
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::empty()),
            false,
            false,
        );
        assert_eq!(
            verify,
            vec![ShellCommand::VerifyInstalled("snake-plus".to_string())]
        );

        let remove = state.handle_key(
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()),
            false,
            false,
        );
        assert_eq!(
            remove,
            vec![ShellCommand::RemoveInstalled("snake-plus".to_string())]
        );
    }

    #[test]
    fn detail_remove_key_emits_operation_command_for_installed_game() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::GameDetail {
            id: "snake-plus".to_string(),
        };
        state.set_installed_game_ids(vec!["snake-plus".to_string()]);

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()),
            false,
            false,
        );

        assert_eq!(
            commands,
            vec![ShellCommand::RemoveInstalled("snake-plus".to_string())]
        );
    }

    #[test]
    fn library_install_key_emits_operation_for_non_installed_game() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Library;

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::empty()),
            false,
            false,
        );

        assert_eq!(
            commands,
            vec![ShellCommand::InstallSelected("snake-plus".to_string())]
        );
    }

    #[test]
    fn library_install_key_ignores_installed_game() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Library;
        state.set_installed_game_ids(vec!["snake-plus".to_string()]);

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::empty()),
            false,
            false,
        );

        assert_eq!(commands, vec![ShellCommand::None]);
    }

    #[test]
    fn detail_install_key_emits_operation_for_non_installed_game() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::GameDetail {
            id: "snake-plus".to_string(),
        };

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::empty()),
            false,
            false,
        );

        assert_eq!(
            commands,
            vec![ShellCommand::InstallSelected("snake-plus".to_string())]
        );
    }

    #[test]
    fn detail_install_key_ignores_installed_game() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::GameDetail {
            id: "snake-plus".to_string(),
        };
        state.set_installed_game_ids(vec!["snake-plus".to_string()]);

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::empty()),
            false,
            false,
        );

        assert_eq!(commands, vec![ShellCommand::None]);
    }

    #[test]
    fn command_palette_executes_route_command() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Settings;
        state.overlay = Some(Overlay::CommandPalette);

        let _ = state.handle_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
            false,
            false,
        );
        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            false,
            false,
        );

        assert_eq!(
            commands,
            vec![
                ShellCommand::OpenRoute(Route::Library),
                ShellCommand::SetOverlay(None),
            ]
        );
    }

    #[test]
    fn command_palette_does_not_directly_leave_running_runner_route() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Runner;
        state.overlay = Some(Overlay::CommandPalette);
        state.command_palette_index = 0; // Open Home

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            true,
            false,
        );

        assert!(
            !commands
                .iter()
                .any(|cmd| matches!(cmd, ShellCommand::OpenRoute(Route::Home))),
            "active runner should require explicit leave confirmation before route switch"
        );
    }

    #[test]
    fn runner_leave_confirm_accepts_and_stops_active_game() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Runner;
        state.overlay = Some(Overlay::RunnerLeaveConfirm);
        state.runner_leave_target = Some(Route::Home);

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            true,
            false,
        );

        assert_eq!(
            commands,
            vec![
                ShellCommand::StopGame,
                ShellCommand::OpenRoute(Route::Home),
                ShellCommand::SetOverlay(None),
            ]
        );
        assert!(state.runner_leave_target.is_none());
    }

    #[test]
    fn runner_leave_confirm_cancel_keeps_runner_active() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Runner;
        state.overlay = Some(Overlay::RunnerLeaveConfirm);
        state.runner_leave_target = Some(Route::Library);

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::empty()),
            true,
            false,
        );

        assert_eq!(commands, vec![ShellCommand::SetOverlay(None)]);
        assert!(state.runner_leave_target.is_none());
    }

    #[test]
    fn search_query_filters_list_and_restores_on_close() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Library;
        state.overlay = Some(Overlay::Search);

        let _ = state.handle_key(
            KeyEvent::new(KeyCode::Char('t'), KeyModifiers::empty()),
            false,
            false,
        );
        let _ = state.handle_key(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::empty()),
            false,
            false,
        );

        let filtered = state.visible_games_for_route(&Route::Library);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "tetris-like");

        let close = state.handle_key(
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
            false,
            false,
        );
        assert_eq!(close, vec![ShellCommand::SetOverlay(None)]);
        assert!(state.search_query.is_empty());

        let restored = state.visible_games_for_route(&Route::Library);
        assert_eq!(restored.len(), 2);
    }

    #[test]
    fn installed_search_filters_by_tags() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Installed;
        state.overlay = Some(Overlay::Search);
        state.set_installed_game_ids(vec!["snake-plus".to_string(), "tetris-like".to_string()]);

        let _ = state.handle_key(
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::empty()),
            false,
            false,
        );
        let _ = state.handle_key(
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::empty()),
            false,
            false,
        );
        let _ = state.handle_key(
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::empty()),
            false,
            false,
        );
        let _ = state.handle_key(
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::empty()),
            false,
            false,
        );
        let _ = state.handle_key(
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::empty()),
            false,
            false,
        );
        let _ = state.handle_key(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::empty()),
            false,
            false,
        );

        let filtered = state.visible_games_for_route(&Route::Installed);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "tetris-like");
    }

    #[test]
    fn runner_modal_overlay_consumes_keys() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Runner;
        state.overlay = Some(Overlay::RunnerQuitConfirm);

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::empty()),
            true,
            false,
        );
        assert_eq!(commands, vec![ShellCommand::None]);
    }

    #[test]
    fn settings_route_emits_revoke_for_selected_permission_entry() {
        let mut state = ShellState::new(sample_games());
        state.route = Route::Settings;
        state.set_permission_audit_entries(vec![PermissionAuditEntry {
            game_id: "remote-wasm".to_string(),
            capability: "net".to_string(),
            decision: "allow".to_string(),
            remembered: true,
        }]);
        state.settings_index = 1;

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            false,
            false,
        );
        assert_eq!(
            commands,
            vec![ShellCommand::RevokePermission {
                game_id: "remote-wasm".to_string(),
                capability: Some("net".to_string())
            }]
        );
    }

    #[test]
    fn permission_prompt_overlay_emits_decision_command() {
        let mut state = ShellState::new(sample_games());
        state.overlay = Some(Overlay::PermissionPrompt);
        state.set_permission_prompt(Some(PermissionPromptState {
            game_id: "remote-wasm".to_string(),
            capability: "net".to_string(),
            prompt: "Allow network?".to_string(),
        }));

        let commands = state.handle_key(
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::empty()),
            true,
            true,
        );
        assert_eq!(
            commands,
            vec![ShellCommand::ResolvePermissionPrompt(
                PermissionPromptAction::AllowAlways
            )]
        );
    }

    #[test]
    fn detail_screen_renders_game_specific_controls() -> std::io::Result<()> {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend)?;
        let mut state = ShellState::new(sample_games());
        state.route = Route::GameDetail {
            id: "tetris-like".to_string(),
        };

        let context = RenderContext::default();
        terminal.draw(|frame| {
            render(frame, &state, &context);
        })?;

        let buffer = terminal.backend().buffer().clone();
        assert_buffer_contains(&buffer, "Rotate: Z/X");
        assert_buffer_contains(&buffer, "Hard drop: Space");
        Ok(())
    }
}
