use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;
use ratatui::style::Color;
use runtime::{Cell, Frame, Game, InitCtx, RuntimeEvent, UpdateCtx};

const MAP_ROWS: [&str; 15] = [
    "###############",
    "#o...........o#",
    "#.###.###.###.#",
    "#.............#",
    "#.###.#.#.###.#",
    "#.....#.#.....#",
    "###.#.#.#.#.###",
    "#...#.....#...#",
    "#.#.#######.#.#",
    "#.#.........#.#",
    "#.###.###.###.#",
    "#.............#",
    "#.###.#.#.###.#",
    "#o....#.#....o#",
    "###############",
];

const PLAYER_START: (i16, i16) = (7, 11);
const GHOST_STARTS: [(i16, i16); 4] = [(7, 7), (6, 7), (8, 7), (7, 6)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tile {
    Wall,
    Empty,
    Pellet,
    Power,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    fn delta(self) -> (i16, i16) {
        match self {
            Self::Up => (0, -1),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
        }
    }

    fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    fn from_key(key: KeyEvent) -> Option<Self> {
        match key.code {
            KeyCode::Up | KeyCode::Char('w') => Some(Self::Up),
            KeyCode::Down | KeyCode::Char('s') => Some(Self::Down),
            KeyCode::Left | KeyCode::Char('a') => Some(Self::Left),
            KeyCode::Right | KeyCode::Char('d') => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GhostKind {
    Blinky,
    Pinky,
    Inky,
    Clyde,
}

impl GhostKind {
    fn glyph(self) -> char {
        match self {
            Self::Blinky => 'B',
            Self::Pinky => 'P',
            Self::Inky => 'I',
            Self::Clyde => 'C',
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Blinky => Color::Red,
            Self::Pinky => Color::Magenta,
            Self::Inky => Color::Cyan,
            Self::Clyde => Color::Yellow,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ghost {
    kind: GhostKind,
    pos: (i16, i16),
    dir: Direction,
    home: (i16, i16),
}

#[derive(Debug)]
pub struct MazeChaseGame {
    rng: Pcg64Mcg,
    tiles: Vec<Vec<Tile>>,
    player: (i16, i16),
    player_dir: Direction,
    next_player_dir: Direction,
    ghosts: [Ghost; 4],
    frightened_ms: u32,
    tick_accum: u32,
    score: i64,
    lives: i16,
    level: u32,
    pellets_remaining: usize,
    finished: bool,
}

impl MazeChaseGame {
    pub fn new(seed: u64) -> Self {
        let mut game = Self {
            rng: Pcg64Mcg::seed_from_u64(seed),
            tiles: Vec::new(),
            player: PLAYER_START,
            player_dir: Direction::Left,
            next_player_dir: Direction::Left,
            ghosts: [
                Ghost {
                    kind: GhostKind::Blinky,
                    pos: GHOST_STARTS[0],
                    dir: Direction::Left,
                    home: GHOST_STARTS[0],
                },
                Ghost {
                    kind: GhostKind::Pinky,
                    pos: GHOST_STARTS[1],
                    dir: Direction::Right,
                    home: GHOST_STARTS[1],
                },
                Ghost {
                    kind: GhostKind::Inky,
                    pos: GHOST_STARTS[2],
                    dir: Direction::Left,
                    home: GHOST_STARTS[2],
                },
                Ghost {
                    kind: GhostKind::Clyde,
                    pos: GHOST_STARTS[3],
                    dir: Direction::Right,
                    home: GHOST_STARTS[3],
                },
            ],
            frightened_ms: 0,
            tick_accum: 0,
            score: 0,
            lives: 3,
            level: 1,
            pellets_remaining: 0,
            finished: false,
        };
        game.reset_level_map();
        game.reset_round_positions();
        game
    }

    fn reset_level_map(&mut self) {
        self.tiles = MAP_ROWS
            .iter()
            .map(|row| {
                row.chars()
                    .map(|ch| match ch {
                        '#' => Tile::Wall,
                        '.' => Tile::Pellet,
                        'o' => Tile::Power,
                        _ => Tile::Empty,
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        self.pellets_remaining = self
            .tiles
            .iter()
            .flat_map(|row| row.iter())
            .filter(|tile| matches!(tile, Tile::Pellet | Tile::Power))
            .count();
    }

    fn reset_round_positions(&mut self) {
        self.player = PLAYER_START;
        self.player_dir = Direction::Left;
        self.next_player_dir = Direction::Left;
        for ghost in &mut self.ghosts {
            ghost.pos = ghost.home;
            ghost.dir = Direction::Left;
        }
    }

    fn map_w(&self) -> i16 {
        i16::try_from(self.tiles.first().map_or(0, Vec::len)).unwrap_or(0)
    }

    fn map_h(&self) -> i16 {
        i16::try_from(self.tiles.len()).unwrap_or(0)
    }

    fn in_bounds(&self, pos: (i16, i16)) -> bool {
        pos.0 >= 0 && pos.1 >= 0 && pos.0 < self.map_w() && pos.1 < self.map_h()
    }

    fn tile_at(&self, pos: (i16, i16)) -> Tile {
        if !self.in_bounds(pos) {
            return Tile::Wall;
        }
        let ux = usize::try_from(pos.0).unwrap_or(0);
        let uy = usize::try_from(pos.1).unwrap_or(0);
        self.tiles[uy][ux]
    }

    fn set_tile(&mut self, pos: (i16, i16), tile: Tile) {
        if !self.in_bounds(pos) {
            return;
        }
        let ux = usize::try_from(pos.0).unwrap_or(0);
        let uy = usize::try_from(pos.1).unwrap_or(0);
        self.tiles[uy][ux] = tile;
    }

    fn can_move(&self, pos: (i16, i16), dir: Direction) -> bool {
        let (dx, dy) = dir.delta();
        let next = (pos.0 + dx, pos.1 + dy);
        !matches!(self.tile_at(next), Tile::Wall)
    }

    fn try_step(&self, pos: (i16, i16), dir: Direction) -> (i16, i16) {
        if self.can_move(pos, dir) {
            let (dx, dy) = dir.delta();
            (pos.0 + dx, pos.1 + dy)
        } else {
            pos
        }
    }

    fn player_step(&mut self) {
        if self.can_move(self.player, self.next_player_dir) {
            self.player_dir = self.next_player_dir;
        }

        self.player = self.try_step(self.player, self.player_dir);

        match self.tile_at(self.player) {
            Tile::Pellet => {
                self.score = self.score.saturating_add(10);
                self.pellets_remaining = self.pellets_remaining.saturating_sub(1);
                self.set_tile(self.player, Tile::Empty);
            }
            Tile::Power => {
                self.score = self.score.saturating_add(50);
                self.pellets_remaining = self.pellets_remaining.saturating_sub(1);
                self.frightened_ms = 6000;
                self.set_tile(self.player, Tile::Empty);
            }
            Tile::Wall | Tile::Empty => {}
        }
    }

    fn ghost_target(&self, ghost: Ghost) -> (i16, i16) {
        let (px, py) = self.player;
        let (pdx, pdy) = self.player_dir.delta();
        match ghost.kind {
            GhostKind::Blinky => (px, py),
            GhostKind::Pinky => (px + 4 * pdx, py + 4 * pdy),
            GhostKind::Inky => {
                let ahead = (px + 2 * pdx, py + 2 * pdy);
                let blinky = self.ghosts[0].pos;
                (
                    ahead.0 + (ahead.0 - blinky.0),
                    ahead.1 + (ahead.1 - blinky.1),
                )
            }
            GhostKind::Clyde => {
                let dist = (ghost.pos.0 - px).abs() + (ghost.pos.1 - py).abs();
                if dist > 6 {
                    (px, py)
                } else {
                    (1, self.map_h().saturating_sub(2))
                }
            }
        }
    }

    fn choose_ghost_direction(&mut self, idx: usize) -> Direction {
        let ghost = self.ghosts[idx];
        let all_dirs = [
            Direction::Up,
            Direction::Left,
            Direction::Down,
            Direction::Right,
        ];
        let mut candidates = all_dirs
            .into_iter()
            .filter(|dir| {
                if !self.can_move(ghost.pos, *dir) {
                    return false;
                }
                *dir != ghost.dir.opposite()
            })
            .collect::<Vec<_>>();

        if candidates.is_empty() {
            candidates = [
                Direction::Up,
                Direction::Left,
                Direction::Down,
                Direction::Right,
            ]
            .into_iter()
            .filter(|dir| self.can_move(ghost.pos, *dir))
            .collect();
        }

        if candidates.is_empty() {
            return ghost.dir;
        }

        if self.frightened_ms > 0 {
            let pick = self.rng.random_range(0..candidates.len());
            return candidates[pick];
        }

        let target = self.ghost_target(ghost);
        candidates
            .into_iter()
            .min_by_key(|dir| {
                let next = self.try_step(ghost.pos, *dir);
                (next.0 - target.0).abs() + (next.1 - target.1).abs()
            })
            .unwrap_or(ghost.dir)
    }

    fn ghost_step(&mut self) {
        for idx in 0..self.ghosts.len() {
            let dir = self.choose_ghost_direction(idx);
            self.ghosts[idx].dir = dir;
            self.ghosts[idx].pos = self.try_step(self.ghosts[idx].pos, dir);
        }
    }

    fn resolve_collisions(&mut self) {
        for ghost in &mut self.ghosts {
            if ghost.pos != self.player {
                continue;
            }

            if self.frightened_ms > 0 {
                self.score = self.score.saturating_add(200);
                ghost.pos = ghost.home;
                ghost.dir = Direction::Left;
                continue;
            }

            self.lives = self.lives.saturating_sub(1);
            if self.lives <= 0 {
                self.finished = true;
            } else {
                self.reset_round_positions();
            }
            self.frightened_ms = 0;
            return;
        }
    }

    fn level_step_interval_ms(&self) -> u32 {
        let speedup = self.level.saturating_sub(1).saturating_mul(8).min(80);
        140_u32.saturating_sub(speedup).max(60)
    }

    fn process_tick(&mut self, dt_ms: u32) {
        if self.finished {
            return;
        }

        self.tick_accum = self.tick_accum.saturating_add(dt_ms);
        while self.tick_accum >= self.level_step_interval_ms() {
            self.tick_accum = self
                .tick_accum
                .saturating_sub(self.level_step_interval_ms());
            self.player_step();
            self.resolve_collisions();
            if self.finished {
                return;
            }
            self.ghost_step();
            self.resolve_collisions();
            if self.finished {
                return;
            }

            if self.pellets_remaining == 0 {
                self.level = self.level.saturating_add(1);
                self.score = self.score.saturating_add(500);
                self.frightened_ms = 0;
                self.reset_level_map();
                self.reset_round_positions();
            }
        }

        self.frightened_ms = self.frightened_ms.saturating_sub(dt_ms);
    }
}

impl Game for MazeChaseGame {
    fn id(&self) -> &'static str {
        crate::MAZE_CHASE_ID
    }

    fn display_name(&self) -> &'static str {
        "Maze Chase"
    }

    fn init(&mut self, _ctx: &InitCtx) -> Result<()> {
        self.score = 0;
        self.level = 1;
        self.lives = 3;
        self.finished = false;
        self.frightened_ms = 0;
        self.tick_accum = 0;
        self.reset_level_map();
        self.reset_round_positions();
        Ok(())
    }

    fn update(&mut self, event: RuntimeEvent, _ctx: &mut UpdateCtx) -> Result<()> {
        match event {
            RuntimeEvent::Input(key) => {
                if let Some(dir) = Direction::from_key(key) {
                    self.next_player_dir = dir;
                }
            }
            RuntimeEvent::Tick { dt_ms } => self.process_tick(dt_ms),
            RuntimeEvent::Resize { .. }
            | RuntimeEvent::FocusGained
            | RuntimeEvent::FocusLost
            | RuntimeEvent::Pause
            | RuntimeEvent::Resume => {}
        }
        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        frame.clear();

        let map_w = u16::try_from(self.map_w()).unwrap_or(0);
        let map_h = u16::try_from(self.map_h()).unwrap_or(0);
        if frame.width < map_w.saturating_add(2) || frame.height < map_h.saturating_add(3) {
            let warning = "TERMINAL TOO SMALL FOR MAZE CHASE";
            let x = frame
                .width
                .saturating_sub(u16::try_from(warning.len()).unwrap_or(0))
                / 2;
            let y = frame.height / 2;
            for (idx, ch) in warning.chars().enumerate() {
                frame.set(
                    x + u16::try_from(idx).unwrap_or(0),
                    y,
                    Cell {
                        glyph: ch,
                        fg: Color::Red,
                        ..Cell::default()
                    },
                );
            }
            return;
        }

        let status = format!(
            "MAZE CHASE  SCORE:{}  LIVES:{}  LVL:{}",
            self.score, self.lives, self.level
        );
        for (idx, ch) in status.chars().enumerate() {
            let x = u16::try_from(idx).unwrap_or(0);
            if x >= frame.width {
                break;
            }
            frame.set(
                x,
                0,
                Cell {
                    glyph: ch,
                    fg: Color::White,
                    ..Cell::default()
                },
            );
        }

        let map_x = 1_u16;
        let map_y = 2_u16;
        for (y, row) in self.tiles.iter().enumerate() {
            for (x, tile) in row.iter().enumerate() {
                let (glyph, fg) = match tile {
                    Tile::Wall => ('#', Color::Blue),
                    Tile::Empty => (' ', Color::Reset),
                    Tile::Pellet => ('.', Color::LightYellow),
                    Tile::Power => ('o', Color::Yellow),
                };
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

        for ghost in &self.ghosts {
            let (glyph, fg) = if self.frightened_ms > 0 {
                ('g', Color::Cyan)
            } else {
                (ghost.kind.glyph(), ghost.kind.color())
            };
            frame.set(
                map_x + u16::try_from(ghost.pos.0).unwrap_or(0),
                map_y + u16::try_from(ghost.pos.1).unwrap_or(0),
                Cell {
                    glyph,
                    fg,
                    ..Cell::default()
                },
            );
        }

        frame.set(
            map_x + u16::try_from(self.player.0).unwrap_or(0),
            map_y + u16::try_from(self.player.1).unwrap_or(0),
            Cell {
                glyph: 'C',
                fg: Color::Yellow,
                ..Cell::default()
            },
        );

        if self.finished {
            let banner = "GAME OVER";
            let x = map_x + map_w.saturating_sub(u16::try_from(banner.len()).unwrap_or(0)) / 2;
            let y = map_y + map_h / 2;
            for (idx, ch) in banner.chars().enumerate() {
                frame.set(
                    x + u16::try_from(idx).unwrap_or(0),
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
        self.score
    }
}

#[cfg(test)]
mod tests {
    use super::{Direction, MazeChaseGame, Tile};
    use runtime::{Game, InitCtx, RuntimeEvent, UpdateCtx};

    fn init_game(seed: u64) -> MazeChaseGame {
        let mut game = MazeChaseGame::new(seed);
        game.init(&InitCtx {
            width: 100,
            height: 40,
            seed,
        })
        .expect("maze chase init should succeed");
        game
    }

    #[test]
    fn power_pellet_enables_frightened_mode() {
        let mut game = init_game(12);
        game.player = (2, 1);
        game.player_dir = Direction::Left;
        game.next_player_dir = Direction::Left;

        let mut ctx = UpdateCtx::new(100, 40);
        game.update(RuntimeEvent::Tick { dt_ms: 200 }, &mut ctx)
            .expect("tick should succeed");

        assert!(game.frightened_ms > 0);
        assert!(game.score >= 50);
    }

    #[test]
    fn level_advances_when_last_pellet_is_collected() {
        let mut game = init_game(21);
        game.level = 3;
        game.pellets_remaining = 1;
        game.player = (2, 1);
        game.player_dir = Direction::Left;
        game.next_player_dir = Direction::Left;
        game.tiles[1][1] = Tile::Power;

        let mut ctx = UpdateCtx::new(100, 40);
        game.update(RuntimeEvent::Tick { dt_ms: 200 }, &mut ctx)
            .expect("tick should succeed");

        assert_eq!(game.level, 4);
        assert!(game.pellets_remaining > 0);
    }

    #[test]
    fn same_seed_produces_same_state_after_identical_ticks() {
        let mut a = init_game(42);
        let mut b = init_game(42);
        let mut a_ctx = UpdateCtx::new(100, 40);
        let mut b_ctx = UpdateCtx::new(100, 40);

        for _ in 0..25 {
            a.update(RuntimeEvent::Tick { dt_ms: 120 }, &mut a_ctx)
                .expect("tick should succeed");
            b.update(RuntimeEvent::Tick { dt_ms: 120 }, &mut b_ctx)
                .expect("tick should succeed");
        }

        assert_eq!(a.player, b.player);
        assert_eq!(a.ghosts, b.ghosts);
        assert_eq!(a.score, b.score);
        assert_eq!(a.level, b.level);
    }
}
