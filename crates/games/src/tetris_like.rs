use std::collections::VecDeque;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;
use ratatui::style::Color;
use runtime::{Cell, Frame, Game, InitCtx, RuntimeEvent, UpdateCtx};

const BOARD_W: usize = 10;
const BOARD_VISIBLE_H: usize = 20;
const BOARD_HIDDEN_H: usize = 4;
const BOARD_H: usize = BOARD_VISIBLE_H + BOARD_HIDDEN_H;

const PREVIEW_LEN: usize = 5;
const LOCK_DELAY_MS: u32 = 500;
const MAX_LOCK_RESETS: u8 = 15;
const LINE_CLEAR_FLASH_MS: u32 = 80;
const FEEDBACK_BANNER_TTL_MS: u32 = 900;
const FEEDBACK_BANNER_FADE_MS: u32 = 250;
const HOLD_DAS_MS: u32 = 140;
const HOLD_ARR_MS: u32 = 30;
const SOFT_DROP_REPEAT_MS: u32 = 20;

const MAX_RENDER_SCALE: u16 = 4;
const MIN_RENDER_W: u16 = 50;
const MIN_RENDER_H: u16 = 22;
const PREVIEW_GAP: u16 = 1;

const BG_FRAME: Color = Color::Rgb(0x07, 0x0B, 0x0D);
const BG_PANEL_CARD: Color = Color::Rgb(0x0E, 0x12, 0x14);
const FG_BORDER_MAIN: Color = Color::Rgb(0x36, 0x3D, 0x41);
const FG_BORDER_SUBTLE: Color = Color::Rgb(0x2A, 0x30, 0x34);
const FG_TEXT_MUTED: Color = Color::Rgb(0x74, 0x7B, 0x80);
const FG_FEEDBACK_DIM: Color = Color::Gray;
const FG_FEEDBACK_FAINT: Color = Color::DarkGray;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeldAction {
    MoveLeft,
    MoveRight,
    SoftDrop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct HeldInputState {
    pressed: bool,
    elapsed_ms: u32,
    first_repeat_done: bool,
    last_pressed_order: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FeedbackBanner {
    text: String,
    color: Color,
    ttl_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingLineClear {
    rows: Vec<usize>,
    ttl_ms: u32,
    t_spin: TSpinKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RenderLayout {
    scale: u16,
    board_x: u16,
    board_y: u16,
    board_outer_w: u16,
    board_outer_h: u16,
    panel_x: u16,
    panel_y: u16,
    panel_w: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PieceKind {
    I,
    O,
    T,
    S,
    Z,
    J,
    L,
}

impl PieceKind {
    const ALL: [Self; 7] = [
        Self::I,
        Self::O,
        Self::T,
        Self::S,
        Self::Z,
        Self::J,
        Self::L,
    ];

    const fn color(self) -> Color {
        match self {
            Self::I => Color::Cyan,
            Self::O => Color::Yellow,
            Self::T => Color::Magenta,
            Self::S => Color::Green,
            Self::Z => Color::Red,
            Self::J => Color::Blue,
            Self::L => Color::LightYellow,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TSpinKind {
    None,
    Mini,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActivePiece {
    kind: PieceKind,
    x: i32,
    y: i32,
    rotation: u8,
}

impl ActivePiece {
    fn cells(self) -> [(i32, i32); 4] {
        let mut out = [(0, 0); 4];
        let states = mino_states(self.kind);
        let rotation = usize::from(self.rotation % 4);
        for (idx, (dx, dy)) in states[rotation].iter().copied().enumerate() {
            out[idx] = (self.x + dx, self.y + dy);
        }
        out
    }

    fn center(self) -> (i32, i32) {
        (self.x + 1, self.y + 1)
    }
}

#[derive(Debug)]
pub struct TetrisLikeGame {
    rng: Pcg64Mcg,
    board: [[Option<PieceKind>; BOARD_W]; BOARD_H],
    active: ActivePiece,
    bag: Vec<PieceKind>,
    next_queue: VecDeque<PieceKind>,
    hold: Option<PieceKind>,
    hold_used: bool,
    held_left: HeldInputState,
    held_right: HeldInputState,
    held_soft_drop: HeldInputState,
    held_press_counter: u64,
    gravity_accum_ms: u32,
    lock_accum_ms: u32,
    lock_resets: u8,
    last_move_was_rotation: bool,
    score: i64,
    lines: u32,
    level: u32,
    combo: i32,
    back_to_back: bool,
    paused: bool,
    feedback_banner: Option<FeedbackBanner>,
    pending_line_clear: Option<PendingLineClear>,
    finished: bool,
}

impl TetrisLikeGame {
    pub fn new(seed: u64) -> Self {
        let mut game = Self {
            rng: Pcg64Mcg::seed_from_u64(seed),
            board: [[None; BOARD_W]; BOARD_H],
            active: ActivePiece {
                kind: PieceKind::I,
                x: 3,
                y: 0,
                rotation: 0,
            },
            bag: Vec::new(),
            next_queue: VecDeque::new(),
            hold: None,
            hold_used: false,
            held_left: HeldInputState::default(),
            held_right: HeldInputState::default(),
            held_soft_drop: HeldInputState::default(),
            held_press_counter: 0,
            gravity_accum_ms: 0,
            lock_accum_ms: 0,
            lock_resets: 0,
            last_move_was_rotation: false,
            score: 0,
            lines: 0,
            level: 1,
            combo: -1,
            back_to_back: false,
            paused: false,
            feedback_banner: None,
            pending_line_clear: None,
            finished: false,
        };

        game.refill_queue();
        game
    }

    fn reset_state(&mut self) {
        self.board = [[None; BOARD_W]; BOARD_H];
        self.bag.clear();
        self.next_queue.clear();
        self.hold = None;
        self.hold_used = false;
        self.held_left = HeldInputState::default();
        self.held_right = HeldInputState::default();
        self.held_soft_drop = HeldInputState::default();
        self.held_press_counter = 0;
        self.gravity_accum_ms = 0;
        self.lock_accum_ms = 0;
        self.lock_resets = 0;
        self.last_move_was_rotation = false;
        self.score = 0;
        self.lines = 0;
        self.level = 1;
        self.combo = -1;
        self.back_to_back = false;
        self.paused = false;
        self.feedback_banner = None;
        self.pending_line_clear = None;
        self.finished = false;
        self.refill_queue();
        self.spawn_piece();
    }

    fn refill_bag(&mut self) {
        if !self.bag.is_empty() {
            return;
        }

        self.bag = PieceKind::ALL.to_vec();
        for idx in (1..self.bag.len()).rev() {
            let swap_idx = self.rng.random_range(0..=idx);
            self.bag.swap(idx, swap_idx);
        }
    }

    fn draw_from_bag(&mut self) -> PieceKind {
        self.refill_bag();
        self.bag.pop().unwrap_or(PieceKind::I)
    }

    fn refill_queue(&mut self) {
        while self.next_queue.len() < PREVIEW_LEN {
            let next = self.draw_from_bag();
            self.next_queue.push_back(next);
        }
    }

    fn spawn_piece(&mut self) {
        self.refill_queue();
        let kind = self
            .next_queue
            .pop_front()
            .unwrap_or_else(|| self.draw_from_bag());
        self.refill_queue();

        self.active = ActivePiece {
            kind,
            x: 3,
            y: 0,
            rotation: 0,
        };
        self.hold_used = false;
        self.lock_accum_ms = 0;
        self.lock_resets = 0;
        self.last_move_was_rotation = false;

        if self.collides(self.active) {
            self.finished = true;
        }
    }

    fn swap_hold(&mut self) {
        if self.hold_used || self.finished {
            return;
        }

        let active_kind = self.active.kind;
        match self.hold {
            Some(held_kind) => {
                self.hold = Some(active_kind);
                self.active = ActivePiece {
                    kind: held_kind,
                    x: 3,
                    y: 0,
                    rotation: 0,
                };
                if self.collides(self.active) {
                    self.finished = true;
                }
            }
            None => {
                self.hold = Some(active_kind);
                self.spawn_piece();
            }
        }

        self.hold_used = true;
        self.last_move_was_rotation = false;
    }

    fn next_press_order(&mut self) -> u64 {
        self.held_press_counter = self.held_press_counter.saturating_add(1);
        self.held_press_counter
    }

    fn clear_held_state(state: &mut HeldInputState) {
        state.pressed = false;
        state.elapsed_ms = 0;
        state.first_repeat_done = false;
    }

    fn press_held_action(&mut self, action: HeldAction) {
        match action {
            HeldAction::MoveLeft => {
                if self.held_left.pressed {
                    return;
                }
                self.held_left.pressed = true;
                self.held_left.elapsed_ms = 0;
                self.held_left.first_repeat_done = false;
                self.held_left.last_pressed_order = self.next_press_order();
                let _ = self.try_move(-1, 0);
            }
            HeldAction::MoveRight => {
                if self.held_right.pressed {
                    return;
                }
                self.held_right.pressed = true;
                self.held_right.elapsed_ms = 0;
                self.held_right.first_repeat_done = false;
                self.held_right.last_pressed_order = self.next_press_order();
                let _ = self.try_move(1, 0);
            }
            HeldAction::SoftDrop => {
                if self.held_soft_drop.pressed {
                    return;
                }
                self.held_soft_drop.pressed = true;
                self.held_soft_drop.elapsed_ms = 0;
                self.held_soft_drop.first_repeat_done = false;
                self.held_soft_drop.last_pressed_order = self.next_press_order();
                self.soft_drop();
            }
        }
    }

    fn release_held_action(&mut self, action: HeldAction) {
        match action {
            HeldAction::MoveLeft => {
                Self::clear_held_state(&mut self.held_left);
                if self.held_right.pressed {
                    self.held_right.elapsed_ms = 0;
                    self.held_right.first_repeat_done = false;
                    let _ = self.try_move(1, 0);
                }
            }
            HeldAction::MoveRight => {
                Self::clear_held_state(&mut self.held_right);
                if self.held_left.pressed {
                    self.held_left.elapsed_ms = 0;
                    self.held_left.first_repeat_done = false;
                    let _ = self.try_move(-1, 0);
                }
            }
            HeldAction::SoftDrop => {
                Self::clear_held_state(&mut self.held_soft_drop);
            }
        }
    }

    fn active_horizontal_action(&self) -> Option<HeldAction> {
        match (self.held_left.pressed, self.held_right.pressed) {
            (true, false) => Some(HeldAction::MoveLeft),
            (false, true) => Some(HeldAction::MoveRight),
            (true, true) => {
                if self.held_left.last_pressed_order >= self.held_right.last_pressed_order {
                    Some(HeldAction::MoveLeft)
                } else {
                    Some(HeldAction::MoveRight)
                }
            }
            (false, false) => None,
        }
    }

    fn apply_held_action(&mut self, action: HeldAction) {
        match action {
            HeldAction::MoveLeft => {
                let _ = self.try_move(-1, 0);
            }
            HeldAction::MoveRight => {
                let _ = self.try_move(1, 0);
            }
            HeldAction::SoftDrop => self.soft_drop(),
        }
    }

    fn process_horizontal_hold(&mut self, dt_ms: u32) {
        let Some(action) = self.active_horizontal_action() else {
            return;
        };

        let (mut elapsed_ms, mut first_repeat_done) = match action {
            HeldAction::MoveLeft => (self.held_left.elapsed_ms, self.held_left.first_repeat_done),
            HeldAction::MoveRight => (
                self.held_right.elapsed_ms,
                self.held_right.first_repeat_done,
            ),
            HeldAction::SoftDrop => (0, false),
        };
        elapsed_ms = elapsed_ms.saturating_add(dt_ms);

        let mut repeats = 0_u16;
        if !first_repeat_done {
            if elapsed_ms >= HOLD_DAS_MS {
                elapsed_ms = elapsed_ms.saturating_sub(HOLD_DAS_MS);
                first_repeat_done = true;
                repeats = repeats.saturating_add(1);
                while elapsed_ms >= HOLD_ARR_MS {
                    elapsed_ms = elapsed_ms.saturating_sub(HOLD_ARR_MS);
                    repeats = repeats.saturating_add(1);
                }
            }
        } else {
            while elapsed_ms >= HOLD_ARR_MS {
                elapsed_ms = elapsed_ms.saturating_sub(HOLD_ARR_MS);
                repeats = repeats.saturating_add(1);
            }
        }

        for _ in 0..repeats {
            self.apply_held_action(action);
        }

        match action {
            HeldAction::MoveLeft => {
                self.held_left.elapsed_ms = elapsed_ms;
                self.held_left.first_repeat_done = first_repeat_done;
            }
            HeldAction::MoveRight => {
                self.held_right.elapsed_ms = elapsed_ms;
                self.held_right.first_repeat_done = first_repeat_done;
            }
            HeldAction::SoftDrop => {}
        }
    }

    fn process_soft_drop_hold(&mut self, dt_ms: u32) {
        if !self.held_soft_drop.pressed {
            return;
        }

        self.held_soft_drop.elapsed_ms = self.held_soft_drop.elapsed_ms.saturating_add(dt_ms);
        while self.held_soft_drop.elapsed_ms >= SOFT_DROP_REPEAT_MS {
            self.held_soft_drop.elapsed_ms = self
                .held_soft_drop
                .elapsed_ms
                .saturating_sub(SOFT_DROP_REPEAT_MS);
            self.soft_drop();
        }
    }

    fn process_held_inputs(&mut self, dt_ms: u32) {
        self.process_horizontal_hold(dt_ms);
        self.process_soft_drop_hold(dt_ms);
    }

    fn collides(&self, piece: ActivePiece) -> bool {
        for (x, y) in piece.cells() {
            if x < 0 || x >= i32::try_from(BOARD_W).unwrap_or(0) {
                return true;
            }
            if y >= i32::try_from(BOARD_H).unwrap_or(0) {
                return true;
            }
            if y >= 0 {
                let ux = usize::try_from(x).unwrap_or(0);
                let uy = usize::try_from(y).unwrap_or(0);
                if self.board[uy][ux].is_some() {
                    return true;
                }
            }
        }

        false
    }

    fn is_grounded(&self) -> bool {
        let moved = ActivePiece {
            y: self.active.y + 1,
            ..self.active
        };
        self.collides(moved)
    }

    fn try_move(&mut self, dx: i32, dy: i32) -> bool {
        let candidate = ActivePiece {
            x: self.active.x + dx,
            y: self.active.y + dy,
            ..self.active
        };

        if self.collides(candidate) {
            return false;
        }

        let was_grounded = self.is_grounded();
        self.active = candidate;
        self.last_move_was_rotation = false;

        if was_grounded {
            self.refresh_lock_delay_on_action();
        }

        true
    }

    fn try_rotate(&mut self, clockwise: bool) -> bool {
        let from = self.active.rotation % 4;
        let to = if clockwise {
            (from + 1) % 4
        } else {
            (from + 3) % 4
        };

        let kicks = kick_offsets(self.active.kind, from, to);
        for (dx, dy) in kicks {
            let candidate = ActivePiece {
                rotation: to,
                x: self.active.x + dx,
                y: self.active.y - dy,
                ..self.active
            };

            if self.collides(candidate) {
                continue;
            }

            let was_grounded = self.is_grounded();
            self.active = candidate;
            self.last_move_was_rotation = true;
            if was_grounded {
                self.refresh_lock_delay_on_action();
            }
            return true;
        }

        false
    }

    fn refresh_lock_delay_on_action(&mut self) {
        if self.lock_resets < MAX_LOCK_RESETS {
            self.lock_accum_ms = 0;
            self.lock_resets = self.lock_resets.saturating_add(1);
        }
    }

    fn soft_drop(&mut self) {
        if self.try_move(0, 1) {
            self.score = self.score.saturating_add(1);
        }
    }

    fn hard_drop(&mut self) {
        let mut steps = 0_i64;
        while self.try_move(0, 1) {
            steps = steps.saturating_add(1);
        }
        self.score = self.score.saturating_add(steps.saturating_mul(2));
        self.lock_piece();
    }

    fn gravity_interval_ms(level: u32) -> u32 {
        let lvl = level.max(1);
        let exponent = f64::from(lvl.saturating_sub(1));
        let raw = (0.8 - (exponent * 0.007)).powf(exponent) * 1000.0;
        raw.max(16.0) as u32
    }

    fn update_level(&mut self) {
        self.level = (self.lines / 10).saturating_add(1);
    }

    fn classify_t_spin(&self, cleared_lines: u8) -> TSpinKind {
        if self.active.kind != PieceKind::T || !self.last_move_was_rotation {
            return TSpinKind::None;
        }

        let (cx, cy) = self.active.center();
        let corners = [
            (cx - 1, cy - 1),
            (cx + 1, cy - 1),
            (cx - 1, cy + 1),
            (cx + 1, cy + 1),
        ];

        let mut occupied = 0_u8;
        for (x, y) in corners {
            if x < 0
                || x >= i32::try_from(BOARD_W).unwrap_or(0)
                || y < 0
                || y >= i32::try_from(BOARD_H).unwrap_or(0)
            {
                occupied = occupied.saturating_add(1);
                continue;
            }

            let ux = usize::try_from(x).unwrap_or(0);
            let uy = usize::try_from(y).unwrap_or(0);
            if self.board[uy][ux].is_some() {
                occupied = occupied.saturating_add(1);
            }
        }

        if occupied < 3 {
            return TSpinKind::None;
        }

        let front_corners = match self.active.rotation % 4 {
            0 => [(cx - 1, cy - 1), (cx + 1, cy - 1)],
            1 => [(cx + 1, cy - 1), (cx + 1, cy + 1)],
            2 => [(cx - 1, cy + 1), (cx + 1, cy + 1)],
            _ => [(cx - 1, cy - 1), (cx - 1, cy + 1)],
        };

        let mut front_occupied = 0_u8;
        for (x, y) in front_corners {
            if x < 0
                || x >= i32::try_from(BOARD_W).unwrap_or(0)
                || y < 0
                || y >= i32::try_from(BOARD_H).unwrap_or(0)
            {
                front_occupied = front_occupied.saturating_add(1);
                continue;
            }

            let ux = usize::try_from(x).unwrap_or(0);
            let uy = usize::try_from(y).unwrap_or(0);
            if self.board[uy][ux].is_some() {
                front_occupied = front_occupied.saturating_add(1);
            }
        }

        if front_occupied == 2 {
            return TSpinKind::Full;
        }

        let _ = cleared_lines;
        TSpinKind::Mini
    }

    fn is_perfect_clear(&self) -> bool {
        self.board.iter().all(|row| row.iter().all(Option::is_none))
    }

    fn apply_scoring(&mut self, cleared_lines: u8, t_spin: TSpinKind, perfect_clear: bool) {
        let level = i64::from(self.level.max(1));
        let difficult = Self::is_difficult_clear(t_spin, cleared_lines);

        let mut base: i64 = match (t_spin, cleared_lines) {
            (TSpinKind::None, 1) => 100,
            (TSpinKind::None, 2) => 300,
            (TSpinKind::None, 3) => 500,
            (TSpinKind::None, 4) => 800,
            (TSpinKind::Mini, 0) => 100,
            (TSpinKind::Mini, 1) => 200,
            (TSpinKind::Mini, 2) => 400,
            (TSpinKind::Full, 0) => 400,
            (TSpinKind::Full, 1) => 800,
            (TSpinKind::Full, 2) => 1200,
            (TSpinKind::Full, 3) => 1600,
            _ => 0,
        };

        if cleared_lines > 0 {
            self.combo = self.combo.saturating_add(1);
            if difficult {
                if self.back_to_back {
                    base = base.saturating_mul(3) / 2;
                }
                self.back_to_back = true;
            } else {
                self.back_to_back = false;
            }

            let combo_bonus = if self.combo > 0 {
                i64::from(self.combo)
                    .saturating_mul(50)
                    .saturating_mul(level)
            } else {
                0
            };
            self.score = self
                .score
                .saturating_add(base.saturating_mul(level))
                .saturating_add(combo_bonus);
        } else {
            self.combo = -1;
            self.score = self.score.saturating_add(base.saturating_mul(level));
        }

        if perfect_clear && cleared_lines > 0 {
            let pc_bonus = match cleared_lines {
                1 => 800,
                2 => 1200,
                3 => 1800,
                4 => 2000,
                _ => 0,
            };
            self.score = self
                .score
                .saturating_add(i64::from(pc_bonus).saturating_mul(level));
        }
    }

    fn is_difficult_clear(t_spin: TSpinKind, cleared_lines: u8) -> bool {
        matches!(t_spin, TSpinKind::Full | TSpinKind::Mini) && cleared_lines > 0
            || (t_spin == TSpinKind::None && cleared_lines == 4)
    }

    fn set_feedback_banner(&mut self, text: String, color: Color) {
        if text.is_empty() {
            return;
        }
        self.feedback_banner = Some(FeedbackBanner {
            text,
            color,
            ttl_ms: FEEDBACK_BANNER_TTL_MS,
        });
    }

    fn update_feedback_banner(&mut self, dt_ms: u32) {
        if let Some(banner) = self.feedback_banner.as_mut() {
            banner.ttl_ms = banner.ttl_ms.saturating_sub(dt_ms);
            if banner.ttl_ms == 0 {
                self.feedback_banner = None;
            }
        }
    }

    fn feedback_color_for_ttl(base: Color, ttl_ms: u32) -> Color {
        if ttl_ms > FEEDBACK_BANNER_FADE_MS {
            base
        } else if ttl_ms > FEEDBACK_BANNER_FADE_MS / 2 {
            FG_FEEDBACK_DIM
        } else {
            FG_FEEDBACK_FAINT
        }
    }

    fn emit_scoring_feedback(
        &mut self,
        cleared_lines: u8,
        t_spin: TSpinKind,
        perfect_clear: bool,
        back_to_back_before: bool,
    ) {
        let mut fragments = Vec::new();
        match t_spin {
            TSpinKind::Full => fragments.push("T-SPIN".to_string()),
            TSpinKind::Mini => fragments.push("T-SPIN MINI".to_string()),
            TSpinKind::None => {}
        }

        let clear_fragment = match cleared_lines {
            1 => Some("SINGLE"),
            2 => Some("DOUBLE"),
            3 => Some("TRIPLE"),
            4 => Some("TETRIS"),
            _ => None,
        };
        if let Some(fragment) = clear_fragment {
            fragments.push(fragment.to_string());
        }

        if Self::is_difficult_clear(t_spin, cleared_lines)
            && back_to_back_before
            && self.back_to_back
        {
            fragments.push("BACK-TO-BACK".to_string());
        }

        if cleared_lines > 0 && self.combo > 0 {
            fragments.push(format!("COMBO x{}", self.combo));
        }

        if perfect_clear && cleared_lines > 0 {
            fragments.push("PERFECT CLEAR".to_string());
        }

        if fragments.is_empty() {
            return;
        }

        let color = if perfect_clear {
            Color::Yellow
        } else if t_spin == TSpinKind::Full {
            Color::Magenta
        } else if cleared_lines == 4 {
            Color::Cyan
        } else {
            Color::White
        };
        self.set_feedback_banner(fragments.join(" | "), color);
    }

    fn full_rows(&self) -> Vec<usize> {
        self.board
            .iter()
            .enumerate()
            .filter_map(|(idx, row)| row.iter().all(Option::is_some).then_some(idx))
            .collect()
    }

    fn clear_rows(&mut self, rows: &[usize]) -> u8 {
        let mut marked = [false; BOARD_H];
        for &row in rows {
            if row < BOARD_H {
                marked[row] = true;
            }
        }

        let mut new_rows = Vec::with_capacity(BOARD_H);
        for (idx, row) in self.board.iter().copied().enumerate() {
            if !marked[idx] {
                new_rows.push(row);
            }
        }

        while new_rows.len() < BOARD_H {
            new_rows.insert(0, [None; BOARD_W]);
        }

        self.board.copy_from_slice(&new_rows);
        let cleared = u8::try_from(rows.iter().filter(|&&row| row < BOARD_H).count()).unwrap_or(0);
        self.lines = self.lines.saturating_add(u32::from(cleared));
        self.update_level();
        cleared
    }

    fn resolve_pending_line_clear(&mut self) {
        let Some(pending) = self.pending_line_clear.take() else {
            return;
        };
        let back_to_back_before = self.back_to_back;
        let cleared = self.clear_rows(&pending.rows);
        let perfect_clear = self.is_perfect_clear();
        self.apply_scoring(cleared, pending.t_spin, perfect_clear);
        self.emit_scoring_feedback(cleared, pending.t_spin, perfect_clear, back_to_back_before);
        if !self.finished {
            self.spawn_piece();
        }
    }

    fn lock_piece(&mut self) {
        for (x, y) in self.active.cells() {
            if x < 0 || x >= i32::try_from(BOARD_W).unwrap_or(0) {
                continue;
            }

            if y < 0 {
                self.finished = true;
                continue;
            }

            if y >= i32::try_from(BOARD_H).unwrap_or(0) {
                self.finished = true;
                continue;
            }

            let ux = usize::try_from(x).unwrap_or(0);
            let uy = usize::try_from(y).unwrap_or(0);
            self.board[uy][ux] = Some(self.active.kind);
        }

        let rows = self.full_rows();
        let cleared_count = u8::try_from(rows.len()).unwrap_or(0);
        let t_spin = self.classify_t_spin(cleared_count);

        self.lock_accum_ms = 0;
        self.lock_resets = 0;
        self.last_move_was_rotation = false;

        if !rows.is_empty() && !self.finished {
            self.pending_line_clear = Some(PendingLineClear {
                rows,
                ttl_ms: LINE_CLEAR_FLASH_MS,
                t_spin,
            });
            return;
        }

        let back_to_back_before = self.back_to_back;
        let perfect_clear = self.is_perfect_clear();
        self.apply_scoring(0, t_spin, perfect_clear);
        self.emit_scoring_feedback(0, t_spin, perfect_clear, back_to_back_before);
        if !self.finished {
            self.spawn_piece();
        }
    }

    fn process_tick(&mut self, dt_ms: u32) {
        self.update_feedback_banner(dt_ms);

        if let Some(pending) = self.pending_line_clear.as_mut() {
            pending.ttl_ms = pending.ttl_ms.saturating_sub(dt_ms);
            if pending.ttl_ms == 0 {
                self.resolve_pending_line_clear();
            }
            return;
        }

        self.process_held_inputs(dt_ms);
        self.gravity_accum_ms = self.gravity_accum_ms.saturating_add(dt_ms);
        let interval = Self::gravity_interval_ms(self.level);

        while self.gravity_accum_ms >= interval {
            self.gravity_accum_ms -= interval;
            if !self.try_move(0, 1) {
                break;
            }
        }

        if self.is_grounded() {
            self.lock_accum_ms = self.lock_accum_ms.saturating_add(dt_ms);
            if self.lock_accum_ms >= LOCK_DELAY_MS {
                self.lock_piece();
            }
        } else {
            self.lock_accum_ms = 0;
        }
    }

    fn handle_input(&mut self, key: KeyEvent) {
        if self.pending_line_clear.is_some() {
            return;
        }

        match key.kind {
            KeyEventKind::Release => match key.code {
                KeyCode::Left | KeyCode::Char('a') => {
                    self.release_held_action(HeldAction::MoveLeft)
                }
                KeyCode::Right | KeyCode::Char('d') => {
                    self.release_held_action(HeldAction::MoveRight)
                }
                KeyCode::Down | KeyCode::Char('s') => {
                    self.release_held_action(HeldAction::SoftDrop)
                }
                _ => {}
            },
            KeyEventKind::Repeat => {}
            _ => match key.code {
                KeyCode::Left | KeyCode::Char('a') => self.press_held_action(HeldAction::MoveLeft),
                KeyCode::Right | KeyCode::Char('d') => {
                    self.press_held_action(HeldAction::MoveRight)
                }
                KeyCode::Down | KeyCode::Char('s') => self.press_held_action(HeldAction::SoftDrop),
                KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('x') => {
                    let _ = self.try_rotate(true);
                }
                KeyCode::Char('z') => {
                    let _ = self.try_rotate(false);
                }
                KeyCode::Char(' ') => self.hard_drop(),
                KeyCode::Char('c') => self.swap_hold(),
                _ => {}
            },
        }
    }

    fn ghost_piece(&self) -> ActivePiece {
        let mut ghost = self.active;
        loop {
            let next = ActivePiece {
                y: ghost.y + 1,
                ..ghost
            };
            if self.collides(next) {
                break;
            }
            ghost = next;
        }
        ghost
    }

    fn write_text_with_bg(frame: &mut Frame, x: u16, y: u16, text: &str, fg: Color, bg: Color) {
        for (idx, ch) in text.chars().enumerate() {
            frame.set(
                x + u16::try_from(idx).unwrap_or(0),
                y,
                Cell {
                    glyph: ch,
                    fg,
                    bg,
                    ..Cell::default()
                },
            );
        }
    }

    fn write_text(frame: &mut Frame, x: u16, y: u16, text: &str, fg: Color) {
        Self::write_text_with_bg(frame, x, y, text, fg, BG_FRAME);
    }

    fn draw_border(frame: &mut Frame, x: u16, y: u16, w: u16, h: u16, fg: Color) {
        if w < 2 || h < 2 {
            return;
        }

        frame.set(
            x,
            y,
            Cell {
                glyph: '┌',
                fg,
                ..Cell::default()
            },
        );
        frame.set(
            x + w - 1,
            y,
            Cell {
                glyph: '┐',
                fg,
                ..Cell::default()
            },
        );
        frame.set(
            x,
            y + h - 1,
            Cell {
                glyph: '└',
                fg,
                ..Cell::default()
            },
        );
        frame.set(
            x + w - 1,
            y + h - 1,
            Cell {
                glyph: '┘',
                fg,
                ..Cell::default()
            },
        );

        for dx in 1..(w - 1) {
            frame.set(
                x + dx,
                y,
                Cell {
                    glyph: '─',
                    fg,
                    ..Cell::default()
                },
            );
            frame.set(
                x + dx,
                y + h - 1,
                Cell {
                    glyph: '─',
                    fg,
                    ..Cell::default()
                },
            );
        }

        for dy in 1..(h - 1) {
            frame.set(
                x,
                y + dy,
                Cell {
                    glyph: '│',
                    fg,
                    ..Cell::default()
                },
            );
            frame.set(
                x + w - 1,
                y + dy,
                Cell {
                    glyph: '│',
                    fg,
                    ..Cell::default()
                },
            );
        }
    }

    fn draw_block_scaled(frame: &mut Frame, x: u16, y: u16, scale: u16, glyph: char, color: Color) {
        let width = scale.saturating_mul(2);
        for dy in 0..scale {
            for dx in 0..width {
                frame.set(
                    x + dx,
                    y + dy,
                    Cell {
                        glyph,
                        fg: color,
                        ..Cell::default()
                    },
                );
            }
        }
    }

    fn preview_bounds(kind: PieceKind) -> (i32, i32, i32, i32) {
        let cells = mino_states(kind)[0];
        let mut min_x = i32::MAX;
        let mut max_x = i32::MIN;
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        for (x, y) in cells {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        (min_x, max_x, min_y, max_y)
    }

    fn preview_cell_scale(area_w: u16, area_h: u16, kind: PieceKind, max_scale: u16) -> u16 {
        let (min_x, max_x, min_y, max_y) = Self::preview_bounds(kind);
        let cells_w = u16::try_from(max_x.saturating_sub(min_x).saturating_add(1)).unwrap_or(1);
        let cells_h = u16::try_from(max_y.saturating_sub(min_y).saturating_add(1)).unwrap_or(1);
        let base_w = cells_w.saturating_mul(2).max(1);
        let by_w = area_w / base_w;
        let by_h = area_h / cells_h.max(1);
        by_w.min(by_h).max(1).min(max_scale.max(1))
    }

    fn draw_piece_preview_centered(
        frame: &mut Frame,
        area_x: u16,
        area_y: u16,
        area_w: u16,
        area_h: u16,
        kind: PieceKind,
        max_scale: u16,
    ) {
        if area_w == 0 || area_h == 0 {
            return;
        }

        let (min_x, max_x, min_y, max_y) = Self::preview_bounds(kind);
        let cells_w = u16::try_from(max_x.saturating_sub(min_x).saturating_add(1)).unwrap_or(1);
        let cells_h = u16::try_from(max_y.saturating_sub(min_y).saturating_add(1)).unwrap_or(1);
        let cell_scale = Self::preview_cell_scale(area_w, area_h, kind, max_scale);
        let cell_w = cell_scale.saturating_mul(2);
        let cell_h = cell_scale;
        let piece_w = cells_w.saturating_mul(cell_w);
        let piece_h = cells_h.saturating_mul(cell_h);

        let origin_x = area_x + area_w.saturating_sub(piece_w) / 2;
        let origin_y = area_y + area_h.saturating_sub(piece_h) / 2;
        let max_x = area_x.saturating_add(area_w);
        let max_y = area_y.saturating_add(area_h);

        for (x, y) in mino_states(kind)[0] {
            let nx = u16::try_from(x.saturating_sub(min_x)).unwrap_or(0);
            let ny = u16::try_from(y.saturating_sub(min_y)).unwrap_or(0);
            let px = origin_x + nx.saturating_mul(cell_w);
            let py = origin_y + ny.saturating_mul(cell_h);
            for dy in 0..cell_h {
                let draw_y = py.saturating_add(dy);
                if draw_y >= max_y {
                    continue;
                }
                for dx in 0..cell_w {
                    let draw_x = px.saturating_add(dx);
                    if draw_x >= max_x {
                        continue;
                    }
                    frame.set(
                        draw_x,
                        draw_y,
                        Cell {
                            glyph: '█',
                            fg: kind.color(),
                            ..Cell::default()
                        },
                    );
                }
            }
        }
    }

    fn fill_rect(frame: &mut Frame, x: u16, y: u16, w: u16, h: u16, bg: Color) {
        for dy in 0..h {
            for dx in 0..w {
                frame.set(
                    x + dx,
                    y + dy,
                    Cell {
                        glyph: ' ',
                        fg: Color::Reset,
                        bg,
                        ..Cell::default()
                    },
                );
            }
        }
    }

    fn write_text_clipped(
        frame: &mut Frame,
        x: u16,
        y: u16,
        max_w: u16,
        text: &str,
        fg: Color,
        bg: Color,
    ) {
        let clipped: String = text.chars().take(usize::from(max_w)).collect();
        Self::write_text_with_bg(frame, x, y, &clipped, fg, bg);
    }

    fn panel_split_widths(panel_inner_w: u16) -> (u16, u16) {
        let stats_w = 11_u16.min(panel_inner_w.saturating_sub(2)).max(8);
        let left_w = panel_inner_w
            .saturating_sub(stats_w)
            .saturating_sub(1)
            .max(12);
        (left_w, stats_w)
    }

    fn next_slot_gap(panel_inner_h: u16, hold_box_h: u16) -> u16 {
        let header_rows = 2_u16.saturating_add(hold_box_h);
        let remaining = panel_inner_h.saturating_sub(header_rows);
        let min_rows = u16::try_from(PREVIEW_LEN).unwrap_or(0).saturating_mul(2);
        let with_gap = min_rows.saturating_add(
            PREVIEW_GAP.saturating_mul(u16::try_from(PREVIEW_LEN.saturating_sub(1)).unwrap_or(0)),
        );
        if remaining >= with_gap {
            PREVIEW_GAP
        } else {
            0
        }
    }

    fn next_slot_height(panel_inner_h: u16, hold_box_h: u16, slot_gap: u16) -> u16 {
        let header_rows = 2_u16.saturating_add(hold_box_h);
        let available = panel_inner_h.saturating_sub(header_rows).saturating_sub(
            slot_gap.saturating_mul(u16::try_from(PREVIEW_LEN.saturating_sub(1)).unwrap_or(0)),
        );
        let per_slot = available / u16::try_from(PREVIEW_LEN).unwrap_or(1);
        per_slot.clamp(2, 7)
    }

    fn render_layout(frame_w: u16, frame_h: u16) -> Option<RenderLayout> {
        if frame_w < MIN_RENDER_W || frame_h < MIN_RENDER_H {
            return None;
        }

        let board_w = u16::try_from(BOARD_W).unwrap_or(10);
        let board_h = u16::try_from(BOARD_VISIBLE_H).unwrap_or(20);

        for scale in (1..=MAX_RENDER_SCALE).rev() {
            let board_outer_w = board_w
                .saturating_mul(scale.saturating_mul(2))
                .saturating_add(2);
            let board_outer_h = board_h.saturating_mul(scale).saturating_add(2);
            let panel_w = 22_u16.saturating_add(scale.saturating_mul(6));
            let content_w = board_outer_w.saturating_add(2).saturating_add(panel_w);
            let content_h = board_outer_h;

            if content_w > frame_w || content_h > frame_h {
                continue;
            }

            let origin_x = frame_w.saturating_sub(content_w) / 2;
            let origin_y = frame_h.saturating_sub(content_h) / 2;
            return Some(RenderLayout {
                scale,
                board_x: origin_x,
                board_y: origin_y,
                board_outer_w,
                board_outer_h,
                panel_x: origin_x + board_outer_w + 2,
                panel_y: origin_y,
                panel_w,
            });
        }

        None
    }

    fn fill_background(frame: &mut Frame) {
        for y in 0..frame.height {
            for x in 0..frame.width {
                frame.set(
                    x,
                    y,
                    Cell {
                        glyph: ' ',
                        fg: Color::Reset,
                        bg: BG_FRAME,
                        ..Cell::default()
                    },
                );
            }
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
        self.reset_state();
        Ok(())
    }

    fn update(&mut self, event: RuntimeEvent, _ctx: &mut UpdateCtx) -> Result<()> {
        match event {
            RuntimeEvent::Input(key) => {
                if !self.finished && !self.paused {
                    self.handle_input(key);
                }
            }
            RuntimeEvent::Tick { dt_ms } => {
                if !self.finished && !self.paused {
                    self.process_tick(dt_ms);
                }
            }
            RuntimeEvent::Pause => {
                self.paused = true;
                Self::clear_held_state(&mut self.held_left);
                Self::clear_held_state(&mut self.held_right);
                Self::clear_held_state(&mut self.held_soft_drop);
            }
            RuntimeEvent::Resume => {
                self.paused = false;
            }
            RuntimeEvent::Resize { .. } | RuntimeEvent::FocusGained | RuntimeEvent::FocusLost => {}
        }

        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        Self::fill_background(frame);

        let Some(layout) = Self::render_layout(frame.width, frame.height) else {
            let warning = "TERMINAL TOO SMALL FOR TETRIS";
            let x = frame
                .width
                .saturating_sub(u16::try_from(warning.len()).unwrap_or(0))
                / 2;
            let y = frame.height / 2;
            Self::write_text(frame, x, y, warning, Color::Red);
            return;
        };

        let board_x = layout.board_x;
        let board_y = layout.board_y;
        let scale = layout.scale;
        let cell_w = scale.saturating_mul(2);
        let cell_h = scale;
        Self::draw_border(
            frame,
            board_x,
            board_y,
            layout.board_outer_w,
            layout.board_outer_h,
            FG_BORDER_MAIN,
        );

        let panel_x = layout.panel_x;
        let panel_y = layout.panel_y;
        Self::draw_border(
            frame,
            panel_x,
            panel_y,
            layout.panel_w,
            layout.board_outer_h,
            FG_BORDER_MAIN,
        );

        if self.pending_line_clear.is_none() {
            let ghost = self.ghost_piece();
            for (x, y) in ghost.cells() {
                let visible_y = y - i32::try_from(BOARD_HIDDEN_H).unwrap_or(0);
                if x < 0
                    || x >= i32::try_from(BOARD_W).unwrap_or(0)
                    || visible_y < 0
                    || visible_y >= i32::try_from(BOARD_VISIBLE_H).unwrap_or(0)
                {
                    continue;
                }
                let px = board_x + 1 + u16::try_from(x).unwrap_or(0).saturating_mul(cell_w);
                let py = board_y + 1 + u16::try_from(visible_y).unwrap_or(0).saturating_mul(cell_h);
                Self::draw_block_scaled(frame, px, py, scale, '░', Color::DarkGray);
            }
        }

        for y in 0..BOARD_VISIBLE_H {
            let by = y + BOARD_HIDDEN_H;
            for x in 0..BOARD_W {
                if let Some(kind) = self.board[by][x] {
                    let px = board_x + 1 + u16::try_from(x).unwrap_or(0).saturating_mul(cell_w);
                    let py = board_y + 1 + u16::try_from(y).unwrap_or(0).saturating_mul(cell_h);
                    Self::draw_block_scaled(frame, px, py, scale, '█', kind.color());
                }
            }
        }

        if let Some(pending) = self.pending_line_clear.as_ref() {
            for &row in &pending.rows {
                if row < BOARD_HIDDEN_H {
                    continue;
                }
                let visible_y = row.saturating_sub(BOARD_HIDDEN_H);
                if visible_y >= BOARD_VISIBLE_H {
                    continue;
                }
                let py = board_y + 1 + u16::try_from(visible_y).unwrap_or(0).saturating_mul(cell_h);
                for x in 0..BOARD_W {
                    let px = board_x + 1 + u16::try_from(x).unwrap_or(0).saturating_mul(cell_w);
                    Self::draw_block_scaled(frame, px, py, scale, '█', Color::White);
                }
            }
        } else {
            for (x, y) in self.active.cells() {
                let visible_y = y - i32::try_from(BOARD_HIDDEN_H).unwrap_or(0);
                if x < 0
                    || x >= i32::try_from(BOARD_W).unwrap_or(0)
                    || visible_y < 0
                    || visible_y >= i32::try_from(BOARD_VISIBLE_H).unwrap_or(0)
                {
                    continue;
                }
                let px = board_x + 1 + u16::try_from(x).unwrap_or(0).saturating_mul(cell_w);
                let py = board_y + 1 + u16::try_from(visible_y).unwrap_or(0).saturating_mul(cell_h);
                Self::draw_block_scaled(frame, px, py, scale, '█', self.active.kind.color());
            }
        }

        let panel_inner_x = panel_x + 1;
        let panel_inner_y = panel_y + 1;
        let panel_inner_w = layout.panel_w.saturating_sub(2);
        let panel_inner_h = layout.board_outer_h.saturating_sub(2);

        let (left_w, stats_w) = Self::panel_split_widths(panel_inner_w);
        let separator_x = panel_inner_x + left_w;
        let right_x = separator_x + 1;
        for y in panel_inner_y..(panel_inner_y + panel_inner_h) {
            frame.set(
                separator_x,
                y,
                Cell {
                    glyph: '│',
                    fg: FG_BORDER_SUBTLE,
                    bg: BG_FRAME,
                    ..Cell::default()
                },
            );
        }

        Self::write_text(frame, panel_inner_x, panel_inner_y, "HOLD", Color::White);
        let hold_box_y = panel_inner_y + 1;
        let hold_box_h = if scale >= 2 { 6 } else { 5 };
        let hold_box_w = left_w.max(10);
        Self::fill_rect(
            frame,
            panel_inner_x,
            hold_box_y,
            hold_box_w,
            hold_box_h,
            BG_PANEL_CARD,
        );
        Self::draw_border(
            frame,
            panel_inner_x,
            hold_box_y,
            hold_box_w,
            hold_box_h,
            FG_BORDER_SUBTLE,
        );
        if let Some(hold) = self.hold {
            Self::draw_piece_preview_centered(
                frame,
                panel_inner_x + 1,
                hold_box_y + 1,
                hold_box_w.saturating_sub(2),
                hold_box_h.saturating_sub(2),
                hold,
                3,
            );
        } else {
            Self::write_text_with_bg(
                frame,
                panel_inner_x + 1,
                hold_box_y + 1,
                "(empty)",
                FG_TEXT_MUTED,
                BG_PANEL_CARD,
            );
        }

        let next_title_y = hold_box_y + hold_box_h + 1;
        Self::write_text(frame, panel_inner_x, next_title_y, "NEXT", Color::White);
        let slot_gap = Self::next_slot_gap(panel_inner_h, hold_box_h);
        let slot_h = Self::next_slot_height(panel_inner_h, hold_box_h, slot_gap);
        let slot_start_y = next_title_y + 1;
        let slot_label_w = 2_u16;
        let slot_x = panel_inner_x + slot_label_w + 1;
        let slot_w = hold_box_w.saturating_sub(slot_label_w + 1).max(6);

        for idx in 0..PREVIEW_LEN {
            let idx_u16 = u16::try_from(idx).unwrap_or(0);
            let slot_y = slot_start_y + idx_u16.saturating_mul(slot_h + slot_gap);
            if slot_y + slot_h > panel_inner_y + panel_inner_h {
                break;
            }

            let slot_label = format!("{}:", idx + 1);
            let slot_label_y = slot_y + slot_h / 2;
            Self::write_text_with_bg(
                frame,
                panel_inner_x,
                slot_label_y,
                &slot_label,
                FG_TEXT_MUTED,
                BG_FRAME,
            );

            Self::fill_rect(frame, slot_x, slot_y, slot_w, slot_h, BG_PANEL_CARD);
            Self::draw_border(frame, slot_x, slot_y, slot_w, slot_h, FG_BORDER_SUBTLE);

            if let Some(kind) = self.next_queue.get(idx).copied() {
                Self::draw_piece_preview_centered(
                    frame,
                    slot_x.saturating_add(1),
                    slot_y.saturating_add(1),
                    slot_w.saturating_sub(2).max(1),
                    slot_h.saturating_sub(2).max(1),
                    kind,
                    3,
                );
            }
        }

        let stats_start_y = panel_inner_y;
        let stats_text_w = stats_w.saturating_sub(1).max(1);
        let next_level = {
            let remainder = self.lines % 10;
            if remainder == 0 { 10 } else { 10 - remainder }
        };

        Self::write_text_clipped(
            frame,
            right_x,
            stats_start_y,
            stats_text_w,
            "SCORE",
            Color::White,
            BG_FRAME,
        );
        Self::write_text_clipped(
            frame,
            right_x,
            stats_start_y + 1,
            stats_text_w,
            &self.score.to_string(),
            Color::LightCyan,
            BG_FRAME,
        );
        Self::write_text_clipped(
            frame,
            right_x,
            stats_start_y + 2,
            stats_text_w,
            "LEVEL",
            Color::White,
            BG_FRAME,
        );
        Self::write_text_clipped(
            frame,
            right_x,
            stats_start_y + 3,
            stats_text_w,
            &self.level.to_string(),
            Color::LightCyan,
            BG_FRAME,
        );
        Self::write_text_clipped(
            frame,
            right_x,
            stats_start_y + 4,
            stats_text_w,
            "LINES",
            Color::White,
            BG_FRAME,
        );
        Self::write_text_clipped(
            frame,
            right_x,
            stats_start_y + 5,
            stats_text_w,
            &self.lines.to_string(),
            Color::LightCyan,
            BG_FRAME,
        );
        Self::write_text_clipped(
            frame,
            right_x,
            stats_start_y + 6,
            stats_text_w,
            "NEXT LVL",
            Color::White,
            BG_FRAME,
        );
        Self::write_text_clipped(
            frame,
            right_x,
            stats_start_y + 7,
            stats_text_w,
            &next_level.to_string(),
            Color::LightCyan,
            BG_FRAME,
        );

        let b2b_text = if self.back_to_back {
            "B2B: ON"
        } else {
            "B2B: OFF"
        };
        let combo_text = if self.combo > 0 {
            format!("COMBO: {}", self.combo)
        } else {
            "COMBO: -".to_string()
        };
        Self::write_text_clipped(
            frame,
            right_x,
            stats_start_y + 9,
            stats_text_w,
            b2b_text,
            Color::White,
            BG_FRAME,
        );
        Self::write_text_clipped(
            frame,
            right_x,
            stats_start_y + 10,
            stats_text_w,
            &combo_text,
            Color::White,
            BG_FRAME,
        );

        if let Some(banner) = self.feedback_banner.as_ref() {
            let feedback_y = stats_start_y + 12;
            if feedback_y + 2 < panel_inner_y + panel_inner_h {
                let feedback_color = Self::feedback_color_for_ttl(banner.color, banner.ttl_ms);
                Self::draw_border(frame, right_x, feedback_y, stats_w, 3, FG_BORDER_SUBTLE);
                Self::write_text_clipped(
                    frame,
                    right_x + 1,
                    feedback_y + 1,
                    stats_w.saturating_sub(2),
                    &banner.text,
                    feedback_color,
                    BG_FRAME,
                );
            }
        }

        let legend_y = panel_inner_y + panel_inner_h.saturating_sub(2);
        Self::write_text_clipped(
            frame,
            panel_inner_x,
            legend_y,
            panel_inner_w,
            "<- -> move  v soft  Space hard",
            FG_TEXT_MUTED,
            BG_FRAME,
        );
        Self::write_text_clipped(
            frame,
            panel_inner_x,
            legend_y + 1,
            panel_inner_w,
            "Z/X rotate  C hold  P pause",
            FG_TEXT_MUTED,
            BG_FRAME,
        );

        if self.finished {
            let alert_h = 3_u16;
            let alert_w = stats_w.max(11);
            let alert_y = panel_inner_y + panel_inner_h.saturating_sub(alert_h + 4);
            Self::draw_border(frame, right_x, alert_y, alert_w, alert_h, Color::Red);
            Self::write_text_clipped(
                frame,
                right_x + 1,
                alert_y + 1,
                alert_w.saturating_sub(2),
                "GAME OVER",
                Color::Red,
                BG_FRAME,
            );
        }

        if self.paused {
            let overlay_text = "PAUSED";
            let overlay_w = 12_u16;
            let overlay_h = 3_u16;
            let overlay_x = board_x + layout.board_outer_w.saturating_sub(overlay_w) / 2;
            let overlay_y = board_y + layout.board_outer_h.saturating_sub(overlay_h) / 2;
            Self::fill_rect(frame, overlay_x, overlay_y, overlay_w, overlay_h, BG_FRAME);
            Self::draw_border(
                frame,
                overlay_x,
                overlay_y,
                overlay_w,
                overlay_h,
                FG_BORDER_MAIN,
            );
            Self::write_text(
                frame,
                overlay_x + 3,
                overlay_y + 1,
                overlay_text,
                Color::White,
            );
        }
    }

    fn is_finished(&self) -> bool {
        self.finished
    }

    fn score(&self) -> i64 {
        self.score
    }
}

fn mino_states(kind: PieceKind) -> &'static [[(i32, i32); 4]; 4] {
    match kind {
        PieceKind::I => &I_STATES,
        PieceKind::O => &O_STATES,
        PieceKind::T => &T_STATES,
        PieceKind::S => &S_STATES,
        PieceKind::Z => &Z_STATES,
        PieceKind::J => &J_STATES,
        PieceKind::L => &L_STATES,
    }
}

fn kick_offsets(kind: PieceKind, from: u8, to: u8) -> &'static [(i32, i32); 5] {
    if kind == PieceKind::O {
        return &KICKS_O;
    }

    if kind == PieceKind::I {
        return match (from % 4, to % 4) {
            (0, 1) => &KICKS_I_01,
            (1, 0) => &KICKS_I_10,
            (1, 2) => &KICKS_I_12,
            (2, 1) => &KICKS_I_21,
            (2, 3) => &KICKS_I_23,
            (3, 2) => &KICKS_I_32,
            (3, 0) => &KICKS_I_30,
            (0, 3) => &KICKS_I_03,
            _ => &KICKS_O,
        };
    }

    match (from % 4, to % 4) {
        (0, 1) => &KICKS_JLSTZ_01,
        (1, 0) => &KICKS_JLSTZ_10,
        (1, 2) => &KICKS_JLSTZ_12,
        (2, 1) => &KICKS_JLSTZ_21,
        (2, 3) => &KICKS_JLSTZ_23,
        (3, 2) => &KICKS_JLSTZ_32,
        (3, 0) => &KICKS_JLSTZ_30,
        (0, 3) => &KICKS_JLSTZ_03,
        _ => &KICKS_O,
    }
}

const I_STATES: [[(i32, i32); 4]; 4] = [
    [(0, 1), (1, 1), (2, 1), (3, 1)],
    [(2, 0), (2, 1), (2, 2), (2, 3)],
    [(0, 2), (1, 2), (2, 2), (3, 2)],
    [(1, 0), (1, 1), (1, 2), (1, 3)],
];

const O_STATES: [[(i32, i32); 4]; 4] = [
    [(1, 0), (2, 0), (1, 1), (2, 1)],
    [(1, 0), (2, 0), (1, 1), (2, 1)],
    [(1, 0), (2, 0), (1, 1), (2, 1)],
    [(1, 0), (2, 0), (1, 1), (2, 1)],
];

const T_STATES: [[(i32, i32); 4]; 4] = [
    [(1, 1), (0, 2), (1, 2), (2, 2)],
    [(1, 1), (1, 2), (2, 2), (1, 3)],
    [(0, 2), (1, 2), (2, 2), (1, 3)],
    [(1, 1), (0, 2), (1, 2), (1, 3)],
];

const S_STATES: [[(i32, i32); 4]; 4] = [
    [(1, 1), (2, 1), (0, 2), (1, 2)],
    [(1, 1), (1, 2), (2, 2), (2, 3)],
    [(1, 2), (2, 2), (0, 3), (1, 3)],
    [(0, 1), (0, 2), (1, 2), (1, 3)],
];

const Z_STATES: [[(i32, i32); 4]; 4] = [
    [(0, 1), (1, 1), (1, 2), (2, 2)],
    [(2, 1), (1, 2), (2, 2), (1, 3)],
    [(0, 2), (1, 2), (1, 3), (2, 3)],
    [(1, 1), (0, 2), (1, 2), (0, 3)],
];

const J_STATES: [[(i32, i32); 4]; 4] = [
    [(0, 1), (0, 2), (1, 2), (2, 2)],
    [(1, 1), (2, 1), (1, 2), (1, 3)],
    [(0, 2), (1, 2), (2, 2), (2, 3)],
    [(1, 1), (1, 2), (0, 3), (1, 3)],
];

const L_STATES: [[(i32, i32); 4]; 4] = [
    [(2, 1), (0, 2), (1, 2), (2, 2)],
    [(1, 1), (1, 2), (1, 3), (2, 3)],
    [(0, 2), (1, 2), (2, 2), (0, 3)],
    [(0, 1), (1, 1), (1, 2), (1, 3)],
];

const KICKS_O: [(i32, i32); 5] = [(0, 0), (0, 0), (0, 0), (0, 0), (0, 0)];

const KICKS_JLSTZ_01: [(i32, i32); 5] = [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)];
const KICKS_JLSTZ_10: [(i32, i32); 5] = [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)];
const KICKS_JLSTZ_12: [(i32, i32); 5] = [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)];
const KICKS_JLSTZ_21: [(i32, i32); 5] = [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)];
const KICKS_JLSTZ_23: [(i32, i32); 5] = [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)];
const KICKS_JLSTZ_32: [(i32, i32); 5] = [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)];
const KICKS_JLSTZ_30: [(i32, i32); 5] = [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)];
const KICKS_JLSTZ_03: [(i32, i32); 5] = [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)];

const KICKS_I_01: [(i32, i32); 5] = [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)];
const KICKS_I_10: [(i32, i32); 5] = [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)];
const KICKS_I_12: [(i32, i32); 5] = [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)];
const KICKS_I_21: [(i32, i32); 5] = [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)];
const KICKS_I_23: [(i32, i32); 5] = [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)];
const KICKS_I_32: [(i32, i32); 5] = [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)];
const KICKS_I_30: [(i32, i32); 5] = [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)];
const KICKS_I_03: [(i32, i32); 5] = [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};
    use runtime::InitCtx;

    use super::*;

    fn test_ctx() -> InitCtx {
        InitCtx {
            width: 80,
            height: 24,
            seed: 1,
        }
    }

    fn filled_cells(game: &TetrisLikeGame) -> usize {
        game.board
            .iter()
            .map(|row| row.iter().filter(|cell| cell.is_some()).count())
            .sum()
    }

    fn frame_text(frame: &Frame) -> String {
        let mut content = String::new();
        for y in 0..frame.height {
            for x in 0..frame.width {
                if let Some(cell) = frame.get(x, y) {
                    content.push(cell.glyph);
                }
            }
            content.push('\n');
        }
        content
    }

    fn key_with_kind(code: KeyCode, kind: KeyEventKind) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn bag_contains_all_seven_pieces_per_cycle() {
        let mut game = TetrisLikeGame::new(42);
        game.bag.clear();

        let mut seen = HashSet::new();
        for _ in 0..7 {
            seen.insert(game.draw_from_bag());
        }

        assert_eq!(seen.len(), 7);
    }

    #[test]
    fn srs_wall_kick_applies_for_t_piece_near_right_wall() {
        let mut game = TetrisLikeGame::new(7);
        game.reset_state();
        game.active = ActivePiece {
            kind: PieceKind::T,
            x: 8,
            y: 8,
            rotation: 0,
        };

        let rotated = game.try_rotate(true);

        assert!(rotated);
        assert_eq!(game.active.rotation, 1);
        assert!(game.active.x < 8);
    }

    #[test]
    fn hold_is_limited_to_once_until_piece_locks() {
        let mut game = TetrisLikeGame::new(99);
        game.reset_state();

        let first_active = game.active.kind;
        game.swap_hold();
        let second_active = game.active.kind;

        assert_eq!(game.hold, Some(first_active));
        assert_ne!(second_active, first_active);

        game.swap_hold();
        assert_eq!(game.hold, Some(first_active));
        assert_eq!(game.active.kind, second_active);

        game.hard_drop();
        game.swap_hold();
        assert_eq!(game.active.kind, first_active);
    }

    #[test]
    fn hard_drop_locks_and_adds_drop_score() {
        let mut game = TetrisLikeGame::new(13);
        game.reset_state();

        let before_cells = filled_cells(&game);
        let before_score = game.score;

        game.hard_drop();

        assert!(filled_cells(&game) > before_cells);
        assert!(game.score > before_score);
    }

    #[test]
    fn t_spin_detection_uses_three_corner_rule() {
        let mut game = TetrisLikeGame::new(1);
        game.reset_state();

        game.active = ActivePiece {
            kind: PieceKind::T,
            x: 4,
            y: 5,
            rotation: 0,
        };
        game.last_move_was_rotation = true;

        let (cx, cy) = game.active.center();
        let corners = [(cx - 1, cy - 1), (cx + 1, cy - 1), (cx - 1, cy + 1)];
        for (x, y) in corners {
            let ux = usize::try_from(x).unwrap_or(0);
            let uy = usize::try_from(y).unwrap_or(0);
            game.board[uy][ux] = Some(PieceKind::Z);
        }

        assert_ne!(game.classify_t_spin(0), TSpinKind::None);
    }

    #[test]
    fn scoring_accounts_for_b2b_combo_and_perfect_clear() {
        let mut game = TetrisLikeGame::new(1);
        game.reset_state();
        game.level = 1;

        game.apply_scoring(4, TSpinKind::None, false);
        assert_eq!(game.score, 800);
        assert!(game.back_to_back);

        game.apply_scoring(4, TSpinKind::None, false);
        assert_eq!(game.score, 2050);

        game.apply_scoring(1, TSpinKind::None, true);
        assert_eq!(game.score, 3050);
    }

    #[test]
    fn level_and_gravity_progression_scale_with_lines() {
        let mut game = TetrisLikeGame::new(1);
        game.reset_state();

        game.lines = 9;
        game.update_level();
        assert_eq!(game.level, 1);

        game.lines = 10;
        game.update_level();
        assert_eq!(game.level, 2);

        assert!(TetrisLikeGame::gravity_interval_ms(1) > TetrisLikeGame::gravity_interval_ms(5));
    }

    #[test]
    fn small_frame_renders_explicit_warning() {
        let mut game = TetrisLikeGame::new(5);
        game.init(&test_ctx()).expect("init should succeed");

        let mut frame = Frame::new(30, 10);
        game.render(&mut frame);

        let content = frame_text(&frame);
        assert!(content.contains("TOO SMALL"));
    }

    #[test]
    fn large_frame_uses_scaled_layout() {
        let layout = TetrisLikeGame::render_layout(120, 52).expect("layout should fit");
        assert_eq!(layout.scale, 2);
    }

    #[test]
    fn tiny_frame_has_no_layout() {
        assert!(TetrisLikeGame::render_layout(40, 20).is_none());
    }

    #[test]
    fn next_slot_height_scales_with_available_panel_space() {
        let wide_gap = TetrisLikeGame::next_slot_gap(40, 6);
        assert_eq!(TetrisLikeGame::next_slot_height(40, 6, wide_gap), 5);
        let tight_gap = TetrisLikeGame::next_slot_gap(20, 5);
        assert_eq!(TetrisLikeGame::next_slot_height(20, 5, tight_gap), 2);
    }

    #[test]
    fn preview_bounds_are_normalized_and_positive() {
        let (min_x, max_x, min_y, max_y) = TetrisLikeGame::preview_bounds(PieceKind::I);
        assert!(max_x >= min_x);
        assert!(max_y >= min_y);
    }

    #[test]
    fn preview_scale_expands_to_fill_available_card_space() {
        assert_eq!(
            TetrisLikeGame::preview_cell_scale(12, 4, PieceKind::S, 3),
            2
        );
        assert_eq!(
            TetrisLikeGame::preview_cell_scale(12, 4, PieceKind::I, 3),
            1
        );
        assert_eq!(
            TetrisLikeGame::preview_cell_scale(24, 6, PieceKind::I, 3),
            3
        );
    }

    #[test]
    fn held_left_repeats_after_das_then_arr() {
        let mut game = TetrisLikeGame::new(77);
        game.reset_state();
        game.active.x = 5;
        game.active.y = 8;

        game.handle_input(key_with_kind(KeyCode::Left, KeyEventKind::Press));
        assert_eq!(game.active.x, 4);

        game.process_tick(139);
        assert_eq!(game.active.x, 4);

        game.process_tick(1);
        assert_eq!(game.active.x, 3);

        game.process_tick(30);
        assert_eq!(game.active.x, 2);
    }

    #[test]
    fn held_right_conflict_uses_latest_press() {
        let mut game = TetrisLikeGame::new(101);
        game.reset_state();
        game.active.x = 5;
        game.active.y = 8;

        game.handle_input(key_with_kind(KeyCode::Left, KeyEventKind::Press));
        assert_eq!(game.active.x, 4);

        game.handle_input(key_with_kind(KeyCode::Right, KeyEventKind::Press));
        assert_eq!(game.active.x, 5);

        game.process_tick(HOLD_DAS_MS);
        assert_eq!(game.active.x, 6);

        game.handle_input(key_with_kind(KeyCode::Right, KeyEventKind::Release));
        assert_eq!(game.active.x, 5);

        game.process_tick(HOLD_DAS_MS);
        assert_eq!(game.active.x, 4);
    }

    #[test]
    fn soft_drop_hold_repeats_without_horizontal_drift() {
        let mut game = TetrisLikeGame::new(202);
        game.reset_state();
        game.active.x = 4;
        game.active.y = 0;

        game.handle_input(key_with_kind(KeyCode::Down, KeyEventKind::Press));
        assert_eq!(game.active.x, 4);
        assert_eq!(game.active.y, 1);

        game.process_tick(SOFT_DROP_REPEAT_MS * 2);
        assert_eq!(game.active.x, 4);
        assert_eq!(game.active.y, 3);
    }

    #[test]
    fn next_slots_render_with_five_bordered_cards() {
        let mut game = TetrisLikeGame::new(3);
        let _ = game.init(&test_ctx());
        let mut frame = Frame::new(140, 46);
        game.render(&mut frame);
        let content = frame_text(&frame);
        assert!(content.contains("1:"));
        assert!(content.contains("2:"));
        assert!(content.contains("3:"));
        assert!(content.contains("4:"));
        assert!(content.contains("5:"));
    }

    #[test]
    fn preview_scale_uses_max_three_when_slot_allows() {
        assert_eq!(
            TetrisLikeGame::preview_cell_scale(24, 8, PieceKind::I, 3),
            3
        );
        assert_eq!(
            TetrisLikeGame::preview_cell_scale(16, 6, PieceKind::I, 3),
            2
        );
        assert_eq!(
            TetrisLikeGame::preview_cell_scale(10, 4, PieceKind::I, 3),
            1
        );
    }

    #[test]
    fn stats_and_next_do_not_overlap_at_120x46() {
        let layout = TetrisLikeGame::render_layout(120, 46);
        assert!(layout.is_some());
        if let Some(layout) = layout {
            let panel_inner_w = layout.panel_w.saturating_sub(2);
            let (left_w, stats_w) = TetrisLikeGame::panel_split_widths(panel_inner_w);
            assert!(left_w + stats_w < panel_inner_w);
        }
    }

    #[test]
    fn stats_and_next_do_not_overlap_at_140x46() {
        let layout = TetrisLikeGame::render_layout(140, 46);
        assert!(layout.is_some());
        if let Some(layout) = layout {
            let panel_inner_w = layout.panel_w.saturating_sub(2);
            let (left_w, stats_w) = TetrisLikeGame::panel_split_widths(panel_inner_w);
            assert!(left_w + stats_w < panel_inner_w);
        }
    }

    #[test]
    fn stats_and_next_do_not_overlap_at_160x50() {
        let layout = TetrisLikeGame::render_layout(160, 50);
        assert!(layout.is_some());
        if let Some(layout) = layout {
            let panel_inner_w = layout.panel_w.saturating_sub(2);
            let (left_w, stats_w) = TetrisLikeGame::panel_split_widths(panel_inner_w);
            assert!(left_w + stats_w < panel_inner_w);
        }
    }

    #[test]
    fn feedback_banner_emits_for_tetris_and_tspin() {
        let mut game = TetrisLikeGame::new(404);
        game.reset_state();

        game.emit_scoring_feedback(4, TSpinKind::None, false, false);
        let tetris_text = game
            .feedback_banner
            .as_ref()
            .map(|banner| banner.text.clone())
            .unwrap_or_default();
        assert!(tetris_text.contains("TETRIS"));

        game.emit_scoring_feedback(1, TSpinKind::Full, false, true);
        let tspin_text = game
            .feedback_banner
            .as_ref()
            .map(|banner| banner.text.clone())
            .unwrap_or_default();
        assert!(tspin_text.contains("T-SPIN"));
    }

    #[test]
    fn line_clear_flash_state_expires_deterministically() {
        let mut game = TetrisLikeGame::new(505);
        game.reset_state();
        let target_row = BOARD_H - 1;
        for x in 0..BOARD_W {
            game.board[target_row][x] = Some(PieceKind::J);
        }
        for x in 3..7 {
            game.board[target_row][x] = None;
        }
        game.active = ActivePiece {
            kind: PieceKind::I,
            x: 3,
            y: 0,
            rotation: 0,
        };

        game.hard_drop();
        assert!(game.pending_line_clear.is_some());

        game.process_tick(LINE_CLEAR_FLASH_MS.saturating_sub(1));
        assert!(game.pending_line_clear.is_some());

        game.process_tick(1);
        assert!(game.pending_line_clear.is_none());
        assert!(game.lines >= 1);
    }

    #[test]
    fn write_text_uses_frame_background_color() {
        let mut frame = Frame::new(24, 8);
        TetrisLikeGame::fill_background(&mut frame);
        TetrisLikeGame::write_text(&mut frame, 2, 2, "SCORE", Color::White);

        let cell = frame.get(2, 2).expect("cell should exist");
        assert_eq!(cell.bg, BG_FRAME);
    }

    #[test]
    fn key_bindings_include_rotation_hold_and_drops() {
        let mut game = TetrisLikeGame::new(88);
        game.reset_state();

        let start_rotation = game.active.rotation;
        game.handle_input(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()));
        assert_ne!(game.active.rotation, start_rotation);

        game.handle_input(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::empty()));
        assert_eq!(game.active.rotation, start_rotation);

        game.handle_input(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::empty()));
        assert!(game.hold.is_some());
    }
}
