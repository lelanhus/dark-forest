use std::collections::VecDeque;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;
use ratatui::style::Color;
use runtime::{Cell, Frame, Game, InitCtx, RuntimeEvent, UpdateCtx};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug)]
pub struct SnakeGame {
    rng: Pcg64Mcg,
    width: u16,
    height: u16,
    snake: VecDeque<(i16, i16)>,
    dir: Direction,
    next_dir: Direction,
    food: (i16, i16),
    tick_accum: u32,
    finished: bool,
}

impl SnakeGame {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Pcg64Mcg::seed_from_u64(seed),
            width: 40,
            height: 18,
            snake: VecDeque::new(),
            dir: Direction::Right,
            next_dir: Direction::Right,
            food: (10, 10),
            tick_accum: 0,
            finished: false,
        }
    }

    fn spawn_food(&mut self) {
        let min_x = 1_i16;
        let min_y = 1_i16;
        let max_x = i16::try_from(self.width.saturating_sub(2)).unwrap_or(1);
        let max_y = i16::try_from(self.height.saturating_sub(2)).unwrap_or(1);

        loop {
            let x = self.rng.random_range(min_x..=max_x);
            let y = self.rng.random_range(min_y..=max_y);
            if !self.snake.contains(&(x, y)) {
                self.food = (x, y);
                break;
            }
        }
    }

    fn apply_input(&mut self, key: KeyEvent) {
        self.next_dir = match key.code {
            KeyCode::Up | KeyCode::Char('w') => Direction::Up,
            KeyCode::Down | KeyCode::Char('s') => Direction::Down,
            KeyCode::Left | KeyCode::Char('a') => Direction::Left,
            KeyCode::Right | KeyCode::Char('d') => Direction::Right,
            _ => self.next_dir,
        };
    }

    fn step(&mut self) {
        if self.finished {
            return;
        }

        let opposite = matches!(
            (self.dir, self.next_dir),
            (Direction::Up, Direction::Down)
                | (Direction::Down, Direction::Up)
                | (Direction::Left, Direction::Right)
                | (Direction::Right, Direction::Left)
        );

        if !opposite {
            self.dir = self.next_dir;
        }

        let Some(&(hx, hy)) = self.snake.front() else {
            return;
        };

        let (dx, dy) = match self.dir {
            Direction::Up => (0, -1),
            Direction::Down => (0, 1),
            Direction::Left => (-1, 0),
            Direction::Right => (1, 0),
        };

        let nx = hx + dx;
        let ny = hy + dy;

        if nx <= 0
            || ny <= 0
            || nx >= i16::try_from(self.width.saturating_sub(1)).unwrap_or(0)
            || ny >= i16::try_from(self.height.saturating_sub(1)).unwrap_or(0)
            || self.snake.contains(&(nx, ny))
        {
            self.finished = true;
            return;
        }

        self.snake.push_front((nx, ny));

        if (nx, ny) == self.food {
            self.spawn_food();
        } else {
            let _ = self.snake.pop_back();
        }
    }
}

impl Game for SnakeGame {
    fn id(&self) -> &'static str {
        crate::SNAKE_ID
    }

    fn display_name(&self) -> &'static str {
        "Snake+"
    }

    fn init(&mut self, ctx: &InitCtx) -> Result<()> {
        self.width = ctx.width.max(20);
        self.height = ctx.height.max(10);
        self.snake.clear();
        let center_x = i16::try_from(self.width / 2).unwrap_or(10);
        let center_y = i16::try_from(self.height / 2).unwrap_or(5);
        self.snake.push_back((center_x - 1, center_y));
        self.snake.push_back((center_x, center_y));
        self.snake.push_back((center_x + 1, center_y));
        self.dir = Direction::Right;
        self.next_dir = Direction::Right;
        self.finished = false;
        self.tick_accum = 0;
        self.spawn_food();
        Ok(())
    }

    fn update(&mut self, event: RuntimeEvent, _ctx: &mut UpdateCtx) -> Result<()> {
        match event {
            RuntimeEvent::Input(key) => self.apply_input(key),
            RuntimeEvent::Tick { dt_ms } => {
                self.tick_accum = self.tick_accum.saturating_add(dt_ms);
                if self.tick_accum >= 120 {
                    self.tick_accum = 0;
                    self.step();
                }
            }
            RuntimeEvent::Resize { w, h } => {
                self.width = w.max(20);
                self.height = h.max(10);
            }
            RuntimeEvent::FocusGained
            | RuntimeEvent::FocusLost
            | RuntimeEvent::Pause
            | RuntimeEvent::Resume => {}
        }
        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let width = frame.width;
        let height = frame.height;

        for x in 0..width {
            frame.set(
                x,
                0,
                Cell {
                    glyph: '#',
                    fg: Color::DarkGray,
                    ..Cell::default()
                },
            );
            frame.set(
                x,
                height.saturating_sub(1),
                Cell {
                    glyph: '#',
                    fg: Color::DarkGray,
                    ..Cell::default()
                },
            );
        }

        for y in 0..height {
            frame.set(
                0,
                y,
                Cell {
                    glyph: '#',
                    fg: Color::DarkGray,
                    ..Cell::default()
                },
            );
            frame.set(
                width.saturating_sub(1),
                y,
                Cell {
                    glyph: '#',
                    fg: Color::DarkGray,
                    ..Cell::default()
                },
            );
        }

        frame.set(
            u16::try_from(self.food.0).unwrap_or(1),
            u16::try_from(self.food.1).unwrap_or(1),
            Cell {
                glyph: '*',
                fg: Color::Yellow,
                ..Cell::default()
            },
        );

        for (idx, (x, y)) in self.snake.iter().enumerate() {
            let glyph = if idx == 0 { '@' } else { 'o' };
            frame.set(
                u16::try_from(*x).unwrap_or(1),
                u16::try_from(*y).unwrap_or(1),
                Cell {
                    glyph,
                    fg: Color::Green,
                    ..Cell::default()
                },
            );
        }

        if self.finished {
            let text = "GAME OVER";
            let x_start = frame
                .width
                .saturating_sub(u16::try_from(text.len()).unwrap_or(0))
                / 2;
            let y = frame.height / 2;
            for (offset, ch) in text.chars().enumerate() {
                frame.set(
                    x_start + u16::try_from(offset).unwrap_or(0),
                    y,
                    Cell {
                        glyph: ch,
                        fg: Color::Red,
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
        i64::try_from(self.snake.len().saturating_sub(3)).unwrap_or(0)
    }
}
