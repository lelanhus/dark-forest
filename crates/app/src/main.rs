use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;
use content::{ContentStore, JsonContentStore};
use crossterm::event::{self, Event as CrosstermEvent, KeyCode};
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{ExecutableCommand, terminal};
use diagnostics::{DiagnosticsSnapshot, PerfDiagnostics, detect_terminal_capabilities};
use games::builtin_catalog;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use registry::{BuiltinRegistry, RegistryProvider};
use runtime::{PerfMode, RunnerSignal, RuntimeEvent, RuntimeRunner};
use shell::{RenderContext, Route, ShellCommand, ShellState};

#[derive(Debug)]
enum AppEvent {
    Terminal(CrosstermEvent),
    Tick,
}

struct AppModel {
    shell: ShellState,
    runner: RuntimeRunner,
    store: JsonContentStore,
    settings: content::Settings,
    play_history: content::PlayHistoryMap,
    current_game_id: Option<String>,
    current_game_started_at: Option<Instant>,
    seed_counter: u64,
}

impl AppModel {
    fn new() -> Result<Self> {
        let listings = builtin_catalog();
        let registry = BuiltinRegistry::new(listings.clone());
        let games = registry
            .list()?
            .into_iter()
            .map(|listing| shell::GameItem {
                id: listing.id,
                name: listing.name,
                description: listing.description,
            })
            .collect::<Vec<_>>();

        let store = JsonContentStore::create_with_default_root()?;
        store.ensure_layout()?;
        let settings = store.load_settings()?;
        let play_history = store.load_play_history()?;

        let (width, height) = terminal::size().unwrap_or((120, 40));
        let mut runner = RuntimeRunner::new(width.saturating_sub(4), height.saturating_sub(8));

        let perf_mode = match settings.performance_mode.as_str() {
            "60" => PerfMode::Fps60,
            "30" => PerfMode::Fps30,
            _ => PerfMode::Auto,
        };
        runner.set_perf_mode(perf_mode);

        Ok(Self {
            shell: ShellState::new(games),
            runner,
            store,
            settings,
            play_history,
            current_game_id: None,
            current_game_started_at: None,
            seed_counter: Utc::now().timestamp() as u64,
        })
    }

    fn next_seed(&mut self) -> u64 {
        self.seed_counter = self.seed_counter.saturating_add(1);
        self.seed_counter
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
        match games::instantiate(game_id, self.next_seed()) {
            Ok(game) => {
                let seed = self.next_seed();
                if let Err(err) = self.runner.start(game, seed) {
                    self.shell.set_error(format!("failed to start game: {err}"));
                    return;
                }
                self.shell.route = Route::Runner;
                self.current_game_id = Some(game_id.to_string());
                self.current_game_started_at = Some(Instant::now());
                self.shell.push_notification(format!("Started {game_id}"));
            }
            Err(err) => self.shell.set_error(format!("unknown game: {err}")),
        }
    }

    fn stop_game(&mut self) {
        self.update_play_stats();
        self.runner.stop();
        self.current_game_id = None;
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
            ShellCommand::None => {}
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
            RunnerSignal::Crashed(err) => self.shell.set_error(format!("Game crashed: {err}")),
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
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();

    run().await
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

    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();
    let running = Arc::new(AtomicBool::new(true));

    let input_running = Arc::clone(&running);
    let input_tx = event_tx.clone();
    let input_task = tokio::task::spawn_blocking(move || {
        while input_running.load(Ordering::SeqCst) {
            if event::poll(Duration::from_millis(20)).unwrap_or(false)
                && let Ok(evt) = event::read()
            {
                let _ = input_tx.send(AppEvent::Terminal(evt));
            }
        }
    });

    let tick_running = Arc::clone(&running);
    let tick_tx = event_tx.clone();
    let tick_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(16));
        while tick_running.load(Ordering::SeqCst) {
            interval.tick().await;
            let _ = tick_tx.send(AppEvent::Tick);
        }
    });

    let mut should_quit = false;
    while !should_quit {
        if let Some(event) = event_rx.recv().await {
            match event {
                AppEvent::Tick => {
                    let _ = model.runner.dispatch(RuntimeEvent::Tick { dt_ms: 16 });

                    if model.runner.is_running() {
                        let _ = model.runner.render();
                    }

                    for signal in model.runner.take_signals() {
                        model.apply_runner_signal(signal);
                    }

                    if model.runner.game_finished() {
                        let previous_game_id = model.current_game_id.clone();
                        model.stop_game();
                        model.shell.route = Route::GameDetail {
                            id: previous_game_id.unwrap_or_else(|| "unknown".to_string()),
                        };
                    }
                }
                AppEvent::Terminal(event) => match event {
                    CrosstermEvent::Key(key) => {
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(event::KeyModifiers::CONTROL)
                        {
                            should_quit = true;
                            continue;
                        }

                        let commands = model.shell.handle_key(key, model.runner.is_running());
                        let forwarded_to_runner =
                            commands.iter().all(|cmd| matches!(cmd, ShellCommand::None))
                                && matches!(model.shell.route, Route::Runner)
                                && model.runner.is_running();

                        for command in commands {
                            if model.handle_shell_command(command) {
                                should_quit = true;
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

    model.update_play_stats();
    model.store.save_settings(&model.settings)?;
    model.store.save_play_history(&model.play_history)?;

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
