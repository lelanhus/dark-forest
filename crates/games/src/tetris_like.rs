use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;
use ratatui::style::Color;
use runtime::{Cell, Frame, Game, InitCtx, RuntimeEvent, UpdateCtx};

const BOARD_W: usize = 10;
const BOARD_H: usize = 20;

const SHAPES: &[&[(i32, i32)]] = &[
    &[(0, 0), (1, 0), (0, 1), (1, 1)],
    &[(-1, 0), (0, 0), (1, 0), (2, 0)],
    &[(-1, 0), (0, 0), (1, 0), (1, 1)],
    &[(-1, 1), (-1, 0), (0, 0), (1, 0)],
    &[(-1, 0), (0, 0), (0, 1), (1, 1)],
];

#[derive(Debug, Clone)]
struct Piece {
    x: i32,
    y: i32,
    shape_idx: usize,
    rotation: u8,
}

#[derive(Debug)]
pub struct TetrisLikeGame {
    rng: Pcg64Mcg,
    board: [[u8; BOARD_W]; BOARD_H],
    active: Piece,
    tick_accum: u32,
    score: i64,
    lines: u32,
    finished: bool,
}

impl TetrisLikeGame {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Pcg64Mcg::seed_from_u64(seed),
            board: [[0; BOARD_W]; BOARD_H],
            active: Piece {
                x: 5,
                y: 0,
                shape_idx: 0,
                rotation: 0,
            },
            tick_accum: 0,
            score: 0,
            lines: 0,
            finished: false,
        }
    }

    fn reset_board(&mut self) {
        self.board = [[0; BOARD_W]; BOARD_H];
        self.score = 0;
        self.lines = 0;
        self.finished = false;
        self.spawn_piece();
    }

    fn spawn_piece(&mut self) {
        self.active = Piece {
            x: i32::try_from(BOARD_W / 2).unwrap_or(5),
            y: 1,
            shape_idx: self.rng.random_range(0..SHAPES.len()),
            rotation: 0,
        };
        if self.collides(self.active.x, self.active.y, self.active.rotation) {
            self.finished = true;
        }
    }

    fn piece_cells(&self, x: i32, y: i32, rot: u8) -> Vec<(i32, i32)> {
        SHAPES[self.active.shape_idx]
            .iter()
            .map(|(px, py)| {
                let (rx, ry) = match rot % 4 {
                    0 => (*px, *py),
                    1 => (-*py, *px),
                    2 => (-*px, -*py),
                    _ => (*py, -*px),
                };
                (x + rx, y + ry)
            })
            .collect()
    }

    fn collides(&self, x: i32, y: i32, rot: u8) -> bool {
        self.piece_cells(x, y, rot).into_iter().any(|(cx, cy)| {
            if cx < 0 || cy < 0 {
                return true;
            }
            let ux = usize::try_from(cx).unwrap_or(usize::MAX);
            let uy = usize::try_from(cy).unwrap_or(usize::MAX);
            ux >= BOARD_W || uy >= BOARD_H || self.board[uy][ux] != 0
        })
    }

    fn lock_piece(&mut self) {
        for (cx, cy) in self.piece_cells(self.active.x, self.active.y, self.active.rotation) {
            if let (Ok(ux), Ok(uy)) = (usize::try_from(cx), usize::try_from(cy))
                && ux < BOARD_W
                && uy < BOARD_H
            {
                self.board[uy][ux] = 1;
            }
        }

        self.clear_lines();
        self.spawn_piece();
    }

    fn clear_lines(&mut self) {
        let mut new_rows = Vec::with_capacity(BOARD_H);
        let mut cleared = 0_u32;

        for row in self.board {
            if row.iter().all(|cell| *cell != 0) {
                cleared = cleared.saturating_add(1);
            } else {
                new_rows.push(row);
            }
        }

        while new_rows.len() < BOARD_H {
            new_rows.insert(0, [0; BOARD_W]);
        }

        self.board.copy_from_slice(&new_rows);
        self.lines = self.lines.saturating_add(cleared);
        self.score = self.score.saturating_add(i64::from(cleared) * 100);
    }

    fn move_horizontal(&mut self, dx: i32) {
        if !self.collides(self.active.x + dx, self.active.y, self.active.rotation) {
            self.active.x += dx;
        }
    }

    fn rotate(&mut self) {
        let next = (self.active.rotation + 1) % 4;
        if !self.collides(self.active.x, self.active.y, next) {
            self.active.rotation = next;
        }
    }

    fn soft_drop(&mut self) {
        if self.collides(self.active.x, self.active.y + 1, self.active.rotation) {
            self.lock_piece();
        } else {
            self.active.y += 1;
        }
    }

    fn handle_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('a') => self.move_horizontal(-1),
            KeyCode::Right | KeyCode::Char('d') => self.move_horizontal(1),
            KeyCode::Up | KeyCode::Char('w') => self.rotate(),
            KeyCode::Down | KeyCode::Char('s') | KeyCode::Char(' ') => self.soft_drop(),
            _ => {}
        }
    }
}

impl Game for TetrisLikeGame {
    fn id(&self) -> &'static str {
        crate::TETRIS_ID
    }

    fn display_name(&self) -> &'static str {
        "Tetris-like"
    }

    fn init(&mut self, _ctx: &InitCtx) -> Result<()> {
        self.reset_board();
        self.tick_accum = 0;
        Ok(())
    }

    fn update(&mut self, event: RuntimeEvent, _ctx: &mut UpdateCtx) -> Result<()> {
        if self.finished {
            return Ok(());
        }

        match event {
            RuntimeEvent::Input(key) => self.handle_input(key),
            RuntimeEvent::Tick { dt_ms } => {
                self.tick_accum = self.tick_accum.saturating_add(dt_ms);
                if self.tick_accum >= 400 {
                    self.tick_accum = 0;
                    self.soft_drop();
                }
            }
            RuntimeEvent::Resize { .. }
            | RuntimeEvent::FocusGained
            | RuntimeEvent::FocusLost
            | RuntimeEvent::Pause
            | RuntimeEvent::Resume => {}
        }

        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let origin_x = 2_u16;
        let origin_y = 1_u16;

        for y in 0..=BOARD_H {
            frame.set(
                origin_x,
                origin_y + u16::try_from(y).unwrap_or(0),
                Cell {
                    glyph: '|',
                    fg: Color::DarkGray,
                    ..Cell::default()
                },
            );
            frame.set(
                origin_x + u16::try_from(BOARD_W + 1).unwrap_or(0),
                origin_y + u16::try_from(y).unwrap_or(0),
                Cell {
                    glyph: '|',
                    fg: Color::DarkGray,
                    ..Cell::default()
                },
            );
        }

        for x in 0..=BOARD_W + 1 {
            frame.set(
                origin_x + u16::try_from(x).unwrap_or(0),
                origin_y + u16::try_from(BOARD_H).unwrap_or(0),
                Cell {
                    glyph: '-',
                    fg: Color::DarkGray,
                    ..Cell::default()
                },
            );
        }

        for y in 0..BOARD_H {
            for x in 0..BOARD_W {
                if self.board[y][x] != 0 {
                    frame.set(
                        origin_x + 1 + u16::try_from(x).unwrap_or(0),
                        origin_y + u16::try_from(y).unwrap_or(0),
                        Cell {
                            glyph: '█',
                            fg: Color::Cyan,
                            ..Cell::default()
                        },
                    );
                }
            }
        }

        for (x, y) in self.piece_cells(self.active.x, self.active.y, self.active.rotation) {
            if x >= 0 && y >= 0 {
                frame.set(
                    origin_x + 1 + u16::try_from(x).unwrap_or(0),
                    origin_y + u16::try_from(y).unwrap_or(0),
                    Cell {
                        glyph: '█',
                        fg: Color::Blue,
                        ..Cell::default()
                    },
                );
            }
        }

        let status = format!("Lines:{} Score:{}", self.lines, self.score);
        for (i, ch) in status.chars().enumerate() {
            frame.set(
                origin_x + u16::try_from(BOARD_W + 4 + i).unwrap_or(0),
                origin_y + 2,
                Cell {
                    glyph: ch,
                    fg: Color::White,
                    ..Cell::default()
                },
            );
        }

        if self.finished {
            let text = "GAME OVER";
            for (i, ch) in text.chars().enumerate() {
                frame.set(
                    origin_x + u16::try_from(BOARD_W + 4 + i).unwrap_or(0),
                    origin_y + 4,
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
        self.score
    }
}
