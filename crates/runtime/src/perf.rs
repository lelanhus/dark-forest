use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfMode {
    Auto,
    Fps60,
    Fps30,
}

impl PerfMode {
    #[must_use]
    pub fn target_fps(self, auto_target: u16) -> u16 {
        match self {
            Self::Auto => auto_target,
            Self::Fps60 => 60,
            Self::Fps30 => 30,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AutoPerfController {
    window: VecDeque<f32>,
    capacity: usize,
    target: u16,
    degrade_counter: u16,
    recover_counter: u16,
}

impl Default for AutoPerfController {
    fn default() -> Self {
        Self {
            window: VecDeque::with_capacity(60),
            capacity: 60,
            target: 60,
            degrade_counter: 0,
            recover_counter: 0,
        }
    }
}

impl AutoPerfController {
    pub fn record_render_ms(&mut self, render_ms: f32) {
        if self.window.len() == self.capacity {
            let _ = self.window.pop_front();
        }
        self.window.push_back(render_ms);

        let avg = self.average_render_ms();
        if self.target == 60 {
            if avg > 10.0 {
                self.degrade_counter = self.degrade_counter.saturating_add(1);
                if self.degrade_counter >= 60 {
                    self.target = 30;
                    self.degrade_counter = 0;
                    self.recover_counter = 0;
                }
            } else {
                self.degrade_counter = 0;
            }
        } else if avg <= 8.0 {
            self.recover_counter = self.recover_counter.saturating_add(1);
            if self.recover_counter >= 180 {
                self.target = 60;
                self.degrade_counter = 0;
                self.recover_counter = 0;
            }
        } else {
            self.recover_counter = 0;
        }
    }

    #[must_use]
    pub fn target_fps(&self) -> u16 {
        self.target
    }

    #[must_use]
    pub fn average_render_ms(&self) -> f32 {
        if self.window.is_empty() {
            return 0.0;
        }

        let sum: f32 = self.window.iter().sum();
        sum / self.window.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::AutoPerfController;

    #[test]
    fn auto_mode_degrades_then_recovers() {
        let mut perf = AutoPerfController::default();

        for _ in 0..80 {
            perf.record_render_ms(12.0);
        }
        assert_eq!(perf.target_fps(), 30);

        for _ in 0..260 {
            perf.record_render_ms(7.0);
        }
        assert_eq!(perf.target_fps(), 60);
    }
}
