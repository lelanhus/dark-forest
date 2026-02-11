use anyhow::Result;
use crossterm::event::KeyCode;
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;
use ratatui::style::Color;
use runtime::{Cell, Frame, Game, InitCtx, RuntimeEvent, UpdateCtx};

#[derive(Debug)]
pub struct MicroRoguelite {
    rng: Pcg64Mcg,
    map_w: i16,
    map_h: i16,
    player: (i16, i16),
    monsters: Vec<(i16, i16)>,
    hp: i16,
    turns: u32,
    log: Vec<String>,
    score: i64,
    finished: bool,
}

impl MicroRoguelite {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Pcg64Mcg::seed_from_u64(seed),
            map_w: 24,
            map_h: 14,
            player: (2, 2),
            monsters: Vec::new(),
            hp: 10,
            turns: 0,
            log: Vec::new(),
            score: 0,
            finished: false,
        }
    }

    fn in_bounds(&self, x: i16, y: i16) -> bool {
        x > 0 && y > 0 && x < self.map_w - 1 && y < self.map_h - 1
    }

    fn spawn_monsters(&mut self) {
        self.monsters.clear();
        for _ in 0..5 {
            loop {
                let x = self.rng.random_range(1..(self.map_w - 1));
                let y = self.rng.random_range(1..(self.map_h - 1));
                if (x, y) != self.player && !self.monsters.contains(&(x, y)) {
                    self.monsters.push((x, y));
                    break;
                }
            }
        }
    }

    fn push_log(&mut self, entry: impl Into<String>) {
        self.log.push(entry.into());
        if self.log.len() > 6 {
            let _ = self.log.remove(0);
        }
    }

    fn move_player(&mut self, dx: i16, dy: i16) {
        if self.finished {
            return;
        }

        let nx = self.player.0 + dx;
        let ny = self.player.1 + dy;
        if !self.in_bounds(nx, ny) {
            return;
        }

        if let Some(idx) = self.monsters.iter().position(|m| *m == (nx, ny)) {
            self.monsters.remove(idx);
            self.score += 10;
            self.push_log("You strike a foe.");
        } else {
            self.player = (nx, ny);
        }

        self.turns = self.turns.saturating_add(1);
        self.enemy_turn();
    }

    fn enemy_turn(&mut self) {
        let target = self.player;
        let mut next_positions = self.monsters.clone();
        let mut messages: Vec<&str> = Vec::new();

        for (idx, monster) in self.monsters.iter().copied().enumerate() {
            let dx = (target.0 - monster.0).signum();
            let dy = (target.1 - monster.1).signum();
            let nx = monster.0 + dx;
            let ny = monster.1 + dy;

            if (nx, ny) == self.player {
                self.hp -= 1;
                messages.push("A foe hits you.");
                if self.hp <= 0 {
                    self.finished = true;
                    messages.push("You were defeated.");
                }
                continue;
            }

            let occupied = next_positions
                .iter()
                .enumerate()
                .any(|(other_idx, pos)| other_idx != idx && *pos == (nx, ny));

            if self.in_bounds(nx, ny) && !occupied {
                next_positions[idx] = (nx, ny);
            }
        }

        self.monsters = next_positions;
        for message in messages {
            self.push_log(message);
        }

        if self.monsters.is_empty() {
            self.finished = true;
            self.score += i64::from(self.hp.max(0)) * 5;
            self.push_log("Area cleared!");
        }
    }
}

impl Game for MicroRoguelite {
    fn id(&self) -> &'static str {
        crate::ROGUELITE_ID
    }

    fn display_name(&self) -> &'static str {
        "Micro Roguelite"
    }

    fn init(&mut self, _ctx: &InitCtx) -> Result<()> {
        self.player = (2, 2);
        self.hp = 10;
        self.turns = 0;
        self.score = 0;
        self.finished = false;
        self.log.clear();
        self.push_log("Explore the room.");
        self.spawn_monsters();
        Ok(())
    }

    fn update(&mut self, event: RuntimeEvent, _ctx: &mut UpdateCtx) -> Result<()> {
        if self.finished {
            return Ok(());
        }

        if let RuntimeEvent::Input(key) = event {
            match key.code {
                KeyCode::Up | KeyCode::Char('w') => self.move_player(0, -1),
                KeyCode::Down | KeyCode::Char('s') => self.move_player(0, 1),
                KeyCode::Left | KeyCode::Char('a') => self.move_player(-1, 0),
                KeyCode::Right | KeyCode::Char('d') => self.move_player(1, 0),
                KeyCode::Char(' ') | KeyCode::Char('.') => {
                    self.turns = self.turns.saturating_add(1);
                    self.push_log("You hold position.");
                    self.enemy_turn();
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let map_x = 1_u16;
        let map_y = 1_u16;

        for y in 0..self.map_h {
            for x in 0..self.map_w {
                let wall = x == 0 || y == 0 || x == self.map_w - 1 || y == self.map_h - 1;
                let glyph = if wall { '#' } else { '.' };
                let fg = if wall { Color::DarkGray } else { Color::Green };
                frame.set(
                    map_x + u16::try_from(x).unwrap_or(0),
                    map_y + u16::try_from(y).unwrap_or(0),
                    Cell {
                        glyph,
                        fg,
                        ..Cell::default()
                    },
                );
            }
        }

        for (mx, my) in &self.monsters {
            frame.set(
                map_x + u16::try_from(*mx).unwrap_or(0),
                map_y + u16::try_from(*my).unwrap_or(0),
                Cell {
                    glyph: 'm',
                    fg: Color::Red,
                    ..Cell::default()
                },
            );
        }

        frame.set(
            map_x + u16::try_from(self.player.0).unwrap_or(0),
            map_y + u16::try_from(self.player.1).unwrap_or(0),
            Cell {
                glyph: '@',
                fg: Color::Yellow,
                ..Cell::default()
            },
        );

        let stats = format!("HP:{} Turns:{} Score:{}", self.hp, self.turns, self.score);
        for (idx, ch) in stats.chars().enumerate() {
            frame.set(
                map_x
                    + u16::try_from(self.map_w + 2 + i16::try_from(idx).unwrap_or(0)).unwrap_or(0),
                map_y + 1,
                Cell {
                    glyph: ch,
                    fg: Color::White,
                    ..Cell::default()
                },
            );
        }

        let controls = "Move: Arrows/WASD  Wait: Space";
        for (idx, ch) in controls.chars().enumerate() {
            frame.set(
                map_x
                    + u16::try_from(self.map_w + 2 + i16::try_from(idx).unwrap_or(0)).unwrap_or(0),
                map_y,
                Cell {
                    glyph: ch,
                    fg: Color::DarkGray,
                    ..Cell::default()
                },
            );
        }

        for (idx, line) in self.log.iter().rev().take(5).enumerate() {
            for (col, ch) in line.chars().enumerate() {
                frame.set(
                    map_x
                        + u16::try_from(self.map_w + 2 + i16::try_from(col).unwrap_or(0))
                            .unwrap_or(0),
                    map_y + 3 + u16::try_from(idx).unwrap_or(0),
                    Cell {
                        glyph: ch,
                        fg: Color::LightCyan,
                        ..Cell::default()
                    },
                );
            }
        }

        if self.finished {
            let banner = "RUN END";
            for (idx, ch) in banner.chars().enumerate() {
                frame.set(
                    map_x
                        + u16::try_from(self.map_w + 2 + i16::try_from(idx).unwrap_or(0))
                            .unwrap_or(0),
                    map_y + 10,
                    Cell {
                        glyph: ch,
                        fg: Color::Magenta,
                        ..Cell::default()
                    },
                );
            }
        }
    }

    fn is_finished(&self) -> bool {
        self.finished
    }

    fn score(&self) -> i64 {
        self.score
    }
}

#[cfg(test)]
mod tests {
    use super::MicroRoguelite;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use runtime::{Game, InitCtx, RuntimeEvent, UpdateCtx};

    #[test]
    fn wait_turn_advances_turn_counter_and_logs_action() {
        let mut game = MicroRoguelite::new(3);
        game.init(&InitCtx {
            width: 80,
            height: 24,
            seed: 3,
        })
        .expect("micro roguelite init should succeed");
        let initial_turns = game.turns;

        let mut ctx = UpdateCtx::new(80, 24);
        game.update(
            RuntimeEvent::Input(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty())),
            &mut ctx,
        )
        .expect("wait input should succeed");

        assert_eq!(game.turns, initial_turns + 1);
        assert!(
            game.log.iter().any(|line| line.contains("hold position")),
            "expected wait action log entry"
        );
    }
}
