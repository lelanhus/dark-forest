use std::collections::VecDeque;
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::KeyEvent;

use crate::{AutoPerfController, Frame, FrameDelta, Game, InitCtx, PerfMode, UpdateCtx};

#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    Input(KeyEvent),
    Tick { dt_ms: u32 },
    Resize { w: u16, h: u16 },
    FocusGained,
    FocusLost,
    Pause,
    Resume,
}

#[derive(Debug, Clone)]
pub enum RunnerCommand {
    Start { game_id: String },
    Stop,
    Restart,
    ToggleFullscreen,
    PauseToggle,
}

#[derive(Debug, Clone)]
pub enum RunnerSignal {
    Started,
    Stopped,
    Paused,
    Resumed,
    Crashed(String),
    PerfModeChanged(PerfMode),
}

pub struct RuntimeRunner {
    game: Option<Box<dyn Game + Send>>,
    game_id: Option<String>,
    frame: Frame,
    previous_frame: Frame,
    fullscreen: bool,
    paused: bool,
    perf_mode: PerfMode,
    auto_perf: AutoPerfController,
    signals: VecDeque<RunnerSignal>,
    last_render_start: Instant,
}

impl RuntimeRunner {
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            game: None,
            game_id: None,
            frame: Frame::new(width, height),
            previous_frame: Frame::new(width, height),
            fullscreen: false,
            paused: false,
            perf_mode: PerfMode::Auto,
            auto_perf: AutoPerfController::default(),
            signals: VecDeque::new(),
            last_render_start: Instant::now(),
        }
    }

    pub fn set_perf_mode(&mut self, mode: PerfMode) {
        self.perf_mode = mode;
        self.signals.push_back(RunnerSignal::PerfModeChanged(mode));
    }

    pub fn start(&mut self, mut game: Box<dyn Game + Send>, seed: u64) -> Result<()> {
        let ctx = InitCtx {
            width: self.frame.width,
            height: self.frame.height,
            seed,
        };
        game.init(&ctx)?;
        self.game_id = Some(game.id().to_string());
        self.game = Some(game);
        self.paused = false;
        self.signals.push_back(RunnerSignal::Started);
        Ok(())
    }

    pub fn stop(&mut self) {
        self.game = None;
        self.game_id = None;
        self.paused = false;
        self.signals.push_back(RunnerSignal::Stopped);
    }

    pub fn restart(&mut self, seed: u64) -> Result<()> {
        if let Some(game) = self.game.as_mut() {
            let ctx = InitCtx {
                width: self.frame.width,
                height: self.frame.height,
                seed,
            };
            game.reset(&ctx)?;
        }
        Ok(())
    }

    pub fn toggle_fullscreen(&mut self) {
        self.fullscreen = !self.fullscreen;
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        let lifecycle_event = if self.paused {
            RuntimeEvent::Pause
        } else {
            RuntimeEvent::Resume
        };
        let _ = self.dispatch(lifecycle_event);
        self.signals.push_back(if self.paused {
            RunnerSignal::Paused
        } else {
            RunnerSignal::Resumed
        });
    }

    pub fn resize(&mut self, w: u16, h: u16) {
        self.frame.resize(w, h);
        self.previous_frame.resize(w, h);
        let _ = self.dispatch(RuntimeEvent::Resize { w, h });
    }

    pub fn dispatch(&mut self, event: RuntimeEvent) -> Result<()> {
        if self.game.is_none() {
            return Ok(());
        }

        if self.paused && matches!(event, RuntimeEvent::Tick { .. }) {
            return Ok(());
        }

        let mut ctx = UpdateCtx::new(self.frame.width, self.frame.height);
        if let Some(game) = self.game.as_mut()
            && let Err(err) = game.update(event, &mut ctx)
        {
            self.signals
                .push_back(RunnerSignal::Crashed(err.to_string()));
            self.stop();
            return Ok(());
        }

        self.signals.extend(ctx.emitted_signals);
        Ok(())
    }

    pub fn render(&mut self) -> FrameDelta {
        self.last_render_start = Instant::now();
        self.frame.clear();
        if let Some(game) = self.game.as_ref() {
            game.render(&mut self.frame);
        }

        let delta = FrameDelta::between(&self.previous_frame, &self.frame);
        self.previous_frame = self.frame.clone();

        let render_ms = self.last_render_start.elapsed().as_secs_f32() * 1000.0;
        if let Some(next_target) = self.auto_perf.record_render_ms(render_ms)
            && self.perf_mode == PerfMode::Auto
        {
            let mode = if next_target >= 60 {
                PerfMode::Fps60
            } else {
                PerfMode::Fps30
            };
            self.signals.push_back(RunnerSignal::PerfModeChanged(mode));
        }

        delta
    }

    #[must_use]
    pub fn current_frame(&self) -> &Frame {
        &self.frame
    }

    #[must_use]
    pub fn take_signals(&mut self) -> Vec<RunnerSignal> {
        self.signals.drain(..).collect()
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.game.is_some()
    }

    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    #[must_use]
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    #[must_use]
    pub fn game_id(&self) -> Option<&str> {
        self.game_id.as_deref()
    }

    #[must_use]
    pub fn target_frame_duration(&self) -> Duration {
        let target = self
            .perf_mode
            .target_fps(self.auto_perf.target_fps())
            .max(1);
        Duration::from_millis((1000_u64 / u64::from(target)).max(1))
    }

    #[must_use]
    pub fn perf_mode(&self) -> PerfMode {
        self.perf_mode
    }

    #[must_use]
    pub fn auto_target_fps(&self) -> u16 {
        self.auto_perf.target_fps()
    }

    #[must_use]
    pub fn average_render_ms(&self) -> f32 {
        self.auto_perf.average_render_ms()
    }

    #[must_use]
    pub fn score(&self) -> i64 {
        self.game.as_ref().map_or(0, |g| g.score())
    }

    #[must_use]
    pub fn game_finished(&self) -> bool {
        self.game.as_ref().is_some_and(|g| g.is_finished())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::Duration;

    use anyhow::Result;

    use crate::{Game, InitCtx, RunnerSignal, RuntimeEvent, UpdateCtx};

    use super::RuntimeRunner;

    struct SlowGame;

    impl Game for SlowGame {
        fn id(&self) -> &'static str {
            "slow-game"
        }

        fn init(&mut self, _ctx: &InitCtx) -> Result<()> {
            Ok(())
        }

        fn update(&mut self, _event: RuntimeEvent, _ctx: &mut UpdateCtx) -> Result<()> {
            Ok(())
        }

        fn render(&self, frame: &mut crate::Frame) {
            std::thread::sleep(Duration::from_millis(12));
            frame.set(
                0,
                0,
                crate::Cell {
                    glyph: 'X',
                    ..crate::Cell::default()
                },
            );
        }
    }

    struct PauseAwareGame {
        pauses: Arc<AtomicUsize>,
        resumes: Arc<AtomicUsize>,
    }

    impl Game for PauseAwareGame {
        fn id(&self) -> &'static str {
            "pause-aware"
        }

        fn init(&mut self, _ctx: &InitCtx) -> Result<()> {
            Ok(())
        }

        fn update(&mut self, event: RuntimeEvent, _ctx: &mut UpdateCtx) -> Result<()> {
            match event {
                RuntimeEvent::Pause => {
                    self.pauses.fetch_add(1, Ordering::SeqCst);
                }
                RuntimeEvent::Resume => {
                    self.resumes.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
            Ok(())
        }

        fn render(&self, frame: &mut crate::Frame) {
            frame.set(
                0,
                0,
                crate::Cell {
                    glyph: 'P',
                    ..crate::Cell::default()
                },
            );
        }
    }

    #[test]
    fn auto_perf_emits_transition_signal() -> Result<()> {
        let mut runner = RuntimeRunner::new(20, 10);
        runner.start(Box::new(SlowGame), 1)?;

        let mut saw_downshift = false;
        for _ in 0..80 {
            runner.dispatch(RuntimeEvent::Tick { dt_ms: 16 })?;
            let _ = runner.render();
            let signals = runner.take_signals();
            if signals.iter().any(|signal| {
                matches!(
                    signal,
                    RunnerSignal::PerfModeChanged(crate::PerfMode::Fps30)
                )
            }) {
                saw_downshift = true;
                break;
            }
        }

        assert!(saw_downshift, "expected Auto mode to transition to 30fps");
        Ok(())
    }

    #[test]
    fn pause_toggle_dispatches_pause_and_resume_events() -> Result<()> {
        let pauses = Arc::new(AtomicUsize::new(0));
        let resumes = Arc::new(AtomicUsize::new(0));
        let mut runner = RuntimeRunner::new(20, 10);
        runner.start(
            Box::new(PauseAwareGame {
                pauses: Arc::clone(&pauses),
                resumes: Arc::clone(&resumes),
            }),
            1,
        )?;

        runner.toggle_pause();
        runner.toggle_pause();

        assert_eq!(pauses.load(Ordering::SeqCst), 1);
        assert_eq!(resumes.load(Ordering::SeqCst), 1);
        Ok(())
    }
}
