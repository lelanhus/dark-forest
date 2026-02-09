use anyhow::Result;

use crate::{Frame, RunnerSignal, RuntimeEvent};

#[derive(Debug, Clone)]
pub struct InitCtx {
    pub width: u16,
    pub height: u16,
    pub seed: u64,
}

#[derive(Debug, Clone)]
pub struct UpdateCtx {
    pub width: u16,
    pub height: u16,
    pub emitted_signals: Vec<RunnerSignal>,
}

impl UpdateCtx {
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            emitted_signals: Vec::new(),
        }
    }

    pub fn emit(&mut self, signal: RunnerSignal) {
        self.emitted_signals.push(signal);
    }
}

pub trait Game {
    fn id(&self) -> &'static str;

    fn display_name(&self) -> &'static str {
        self.id()
    }

    fn init(&mut self, ctx: &InitCtx) -> Result<()>;

    fn update(&mut self, event: RuntimeEvent, ctx: &mut UpdateCtx) -> Result<()>;

    fn render(&self, frame: &mut Frame);

    fn is_finished(&self) -> bool {
        false
    }

    fn score(&self) -> i64 {
        0
    }

    fn reset(&mut self, ctx: &InitCtx) -> Result<()> {
        self.init(ctx)
    }
}
