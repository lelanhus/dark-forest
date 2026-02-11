use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;
use ratatui::style::Color;
use runtime::{Cell, Frame, Game, InitCtx, RuntimeEvent, UpdateCtx};

const FIELD_W: i16 = 40;
const FIELD_H: i16 = 24;
const ALIEN_ROWS: usize = 5;
const ALIEN_COLS: usize = 11;
const BULLET_STEP_MS: u32 = 50;
const BASE_PLAYER_COOLDOWN_MS: u32 = 180;
const RAPID_FIRE_COOLDOWN_MS: u32 = 80;
const RAPID_FIRE_DURATION_MS: u32 = 6000;
const BASE_PLAYER_MAX_SHOTS: usize = 1;
const RAPID_FIRE_MAX_SHOTS: usize = 3;
const UFO_MOVE_INTERVAL_MS: u32 = 120;
const POWER_UP_FALL_INTERVAL_MS: u32 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bullet {
    x: i16,
    y: i16,
    dy: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShieldCell {
    x: i16,
    y: i16,
    hp: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Ufo {
    x: i16,
    dir: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PowerUpKind {
    RapidFire,
    ShieldRepair,
}

impl PowerUpKind {
    fn next(self) -> Self {
        match self {
            Self::RapidFire => Self::ShieldRepair,
            Self::ShieldRepair => Self::RapidFire,
        }
    }

    fn glyph(self) -> char {
        match self {
            Self::RapidFire => 'R',
            Self::ShieldRepair => 'S',
        }
    }

    fn color(self) -> Color {
        match self {
            Self::RapidFire => Color::LightRed,
            Self::ShieldRepair => Color::LightCyan,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PowerUpDrop {
    x: i16,
    y: i16,
    kind: PowerUpKind,
}

#[derive(Debug)]
pub struct GalacticInvadersGame {
    rng: Pcg64Mcg,
    aliens: [[bool; ALIEN_COLS]; ALIEN_ROWS],
    formation_x: i16,
    formation_y: i16,
    formation_dir: i16,
    move_accum_ms: u32,
    move_interval_ms: u32,
    bullet_accum_ms: u32,
    enemy_shot_accum_ms: u32,
    player_x: i16,
    player_cooldown_ms: u32,
    player_bullets: Vec<Bullet>,
    enemy_bullets: Vec<Bullet>,
    shields: Vec<ShieldCell>,
    ufo: Option<Ufo>,
    ufo_spawn_accum_ms: u32,
    ufo_move_accum_ms: u32,
    power_up_drop: Option<PowerUpDrop>,
    power_up_fall_accum_ms: u32,
    next_ufo_power_up: PowerUpKind,
    rapid_fire_ms: u32,
    score: i64,
    lives: i16,
    wave: u32,
    finished: bool,
}

impl GalacticInvadersGame {
    pub fn new(seed: u64) -> Self {
        let mut game = Self {
            rng: Pcg64Mcg::seed_from_u64(seed),
            aliens: [[true; ALIEN_COLS]; ALIEN_ROWS],
            formation_x: 8,
            formation_y: 2,
            formation_dir: 1,
            move_accum_ms: 0,
            move_interval_ms: 420,
            bullet_accum_ms: 0,
            enemy_shot_accum_ms: 0,
            player_x: FIELD_W / 2,
            player_cooldown_ms: 0,
            player_bullets: Vec::new(),
            enemy_bullets: Vec::new(),
            shields: Vec::new(),
            ufo: None,
            ufo_spawn_accum_ms: 0,
            ufo_move_accum_ms: 0,
            power_up_drop: None,
            power_up_fall_accum_ms: 0,
            next_ufo_power_up: PowerUpKind::RapidFire,
            rapid_fire_ms: 0,
            score: 0,
            lives: 3,
            wave: 1,
            finished: false,
        };
        game.reset_wave();
        game
    }

    fn reset_shields(&mut self) {
        self.shields.clear();
        let bases = [8_i16, 17_i16, 26_i16];
        for base_x in bases {
            for dy in 0..2 {
                for dx in 0..5 {
                    self.shields.push(ShieldCell {
                        x: base_x + dx,
                        y: FIELD_H - 6 + dy,
                        hp: 2,
                    });
                }
            }
        }
    }

    fn reset_wave(&mut self) {
        self.aliens = [[true; ALIEN_COLS]; ALIEN_ROWS];
        self.formation_x = 8;
        self.formation_y = 2;
        self.formation_dir = 1;
        self.move_accum_ms = 0;
        self.enemy_shot_accum_ms = 0;
        self.player_bullets.clear();
        self.enemy_bullets.clear();
        self.ufo = None;
        self.ufo_spawn_accum_ms = 0;
        self.ufo_move_accum_ms = 0;
        self.power_up_drop = None;
        self.power_up_fall_accum_ms = 0;
        self.player_x = FIELD_W / 2;
        self.player_cooldown_ms = 0;
        self.reset_shields();
    }

    fn player_fire_cooldown_ms(&self) -> u32 {
        if self.rapid_fire_ms > 0 {
            RAPID_FIRE_COOLDOWN_MS
        } else {
            BASE_PLAYER_COOLDOWN_MS
        }
    }

    fn max_player_shots(&self) -> usize {
        if self.rapid_fire_ms > 0 {
            RAPID_FIRE_MAX_SHOTS
        } else {
            BASE_PLAYER_MAX_SHOTS
        }
    }

    fn enemy_shot_cadence_ms(&self) -> u32 {
        let total = u32::try_from(ALIEN_ROWS * ALIEN_COLS).unwrap_or(0);
        let live = u32::try_from(self.live_aliens()).unwrap_or(0);
        let eliminated = total.saturating_sub(live);
        let wave_pressure = self.wave.saturating_sub(1).saturating_mul(24);
        let swarm_pressure = eliminated.saturating_mul(4);
        420_u32
            .saturating_sub(wave_pressure.saturating_add(swarm_pressure))
            .max(110)
    }

    fn current_move_interval_ms(&self) -> u32 {
        let total = u32::try_from(ALIEN_ROWS * ALIEN_COLS).unwrap_or(0);
        let live = u32::try_from(self.live_aliens()).unwrap_or(0);
        let eliminated = total.saturating_sub(live);
        self.move_interval_ms
            .saturating_sub(eliminated.saturating_mul(4))
            .max(70)
    }

    fn live_aliens(&self) -> usize {
        self.aliens
            .iter()
            .flat_map(|row| row.iter())
            .filter(|alive| **alive)
            .count()
    }

    fn alien_bounds(&self) -> Option<(i16, i16, i16)> {
        let mut min_x = i16::MAX;
        let mut max_x = i16::MIN;
        let mut max_y = i16::MIN;

        for row in 0..ALIEN_ROWS {
            for col in 0..ALIEN_COLS {
                if !self.aliens[row][col] {
                    continue;
                }
                let x = self.formation_x + i16::try_from(col).unwrap_or(0) * 2;
                let y = self.formation_y + i16::try_from(row).unwrap_or(0);
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }

        if min_x == i16::MAX {
            None
        } else {
            Some((min_x, max_x, max_y))
        }
    }

    fn move_formation(&mut self) {
        let Some((min_x, max_x, _)) = self.alien_bounds() else {
            return;
        };

        let would_hit_wall = if self.formation_dir > 0 {
            max_x + self.formation_dir >= FIELD_W - 1
        } else {
            min_x + self.formation_dir <= 1
        };

        if would_hit_wall {
            self.formation_dir *= -1;
            self.formation_y = self.formation_y.saturating_add(1);
        } else {
            self.formation_x += self.formation_dir;
        }
    }

    fn shoot_enemy_bullet(&mut self) {
        let mut eligible = Vec::new();
        for col in 0..ALIEN_COLS {
            for row in (0..ALIEN_ROWS).rev() {
                if self.aliens[row][col] {
                    eligible.push((row, col));
                    break;
                }
            }
        }

        if eligible.is_empty() {
            return;
        }

        let pick = self.rng.random_range(0..eligible.len());
        let (row, col) = eligible[pick];
        self.enemy_bullets.push(Bullet {
            x: self.formation_x + i16::try_from(col).unwrap_or(0) * 2,
            y: self.formation_y + i16::try_from(row).unwrap_or(0) + 1,
            dy: 1,
        });
    }

    fn maybe_enemy_shoot(&mut self, dt_ms: u32) {
        self.enemy_shot_accum_ms = self.enemy_shot_accum_ms.saturating_add(dt_ms);
        let cadence = self.enemy_shot_cadence_ms();
        while self.enemy_shot_accum_ms >= cadence {
            self.enemy_shot_accum_ms = self.enemy_shot_accum_ms.saturating_sub(cadence);
            self.shoot_enemy_bullet();
        }
    }

    fn maybe_spawn_ufo(&mut self, dt_ms: u32) {
        self.ufo_spawn_accum_ms = self.ufo_spawn_accum_ms.saturating_add(dt_ms);
        if self.ufo.is_some() || self.ufo_spawn_accum_ms < 9000 {
            return;
        }
        self.ufo_spawn_accum_ms = 0;

        let spawn_left = self.rng.random::<bool>();
        self.ufo = Some(Ufo {
            x: if spawn_left { 1 } else { FIELD_W - 2 },
            dir: if spawn_left { 1 } else { -1 },
        });
    }

    fn step_ufo(&mut self, dt_ms: u32) {
        if self.ufo.is_none() {
            self.ufo_move_accum_ms = 0;
            return;
        }

        self.ufo_move_accum_ms = self.ufo_move_accum_ms.saturating_add(dt_ms);
        while self.ufo_move_accum_ms >= UFO_MOVE_INTERVAL_MS {
            self.ufo_move_accum_ms = self.ufo_move_accum_ms.saturating_sub(UFO_MOVE_INTERVAL_MS);
            if let Some(mut ufo) = self.ufo {
                ufo.x += ufo.dir;
                if ufo.x <= 0 || ufo.x >= FIELD_W - 1 {
                    self.ufo = None;
                    self.ufo_move_accum_ms = 0;
                    break;
                }
                self.ufo = Some(ufo);
            }
        }
    }

    fn queue_ufo_power_up_drop(&mut self, x: i16) {
        let kind = self.next_ufo_power_up;
        self.next_ufo_power_up = self.next_ufo_power_up.next();
        self.power_up_drop = Some(PowerUpDrop { x, y: 2, kind });
        self.power_up_fall_accum_ms = 0;
    }

    fn apply_power_up(&mut self, kind: PowerUpKind) {
        match kind {
            PowerUpKind::RapidFire => {
                self.rapid_fire_ms = RAPID_FIRE_DURATION_MS;
            }
            PowerUpKind::ShieldRepair => self.reset_shields(),
        }
    }

    fn clear_active_powerups(&mut self) {
        self.rapid_fire_ms = 0;
        self.power_up_drop = None;
        self.power_up_fall_accum_ms = 0;
    }

    fn step_power_up_drop(&mut self, dt_ms: u32) {
        let Some(mut drop) = self.power_up_drop else {
            self.power_up_fall_accum_ms = 0;
            return;
        };

        self.power_up_fall_accum_ms = self.power_up_fall_accum_ms.saturating_add(dt_ms);
        while self.power_up_fall_accum_ms >= POWER_UP_FALL_INTERVAL_MS {
            self.power_up_fall_accum_ms = self
                .power_up_fall_accum_ms
                .saturating_sub(POWER_UP_FALL_INTERVAL_MS);
            drop.y = drop.y.saturating_add(1);

            if drop.y >= FIELD_H - 1 {
                self.power_up_drop = None;
                return;
            }

            if drop.y == FIELD_H - 2 && drop.x == self.player_x {
                self.apply_power_up(drop.kind);
                self.power_up_drop = None;
                return;
            }
        }

        self.power_up_drop = Some(drop);
    }

    fn damage_shield(&mut self, x: i16, y: i16) -> bool {
        if let Some(idx) = self
            .shields
            .iter()
            .position(|cell| cell.x == x && cell.y == y)
        {
            if self.shields[idx].hp > 0 {
                self.shields[idx].hp -= 1;
            }
            if self.shields[idx].hp == 0 {
                self.shields.remove(idx);
            }
            return true;
        }
        false
    }

    fn step_bullets(&mut self) {
        for bullet in &mut self.player_bullets {
            bullet.y += bullet.dy;
        }
        for bullet in &mut self.enemy_bullets {
            bullet.y += bullet.dy;
        }

        self.player_bullets.retain(|b| b.y > 0 && b.y < FIELD_H - 1);
        self.enemy_bullets.retain(|b| b.y > 0 && b.y < FIELD_H - 1);

        let mut next_player = Vec::new();
        let player_bullets = std::mem::take(&mut self.player_bullets);
        for bullet in player_bullets {
            if let Some(ufo) = self.ufo
                && bullet.y == 1
                && bullet.x == ufo.x
            {
                self.score = self.score.saturating_add(150);
                self.queue_ufo_power_up_drop(ufo.x);
                self.ufo = None;
                continue;
            }

            if self.damage_shield(bullet.x, bullet.y) {
                continue;
            }

            let mut hit_alien = false;
            for row in 0..ALIEN_ROWS {
                for col in 0..ALIEN_COLS {
                    if !self.aliens[row][col] {
                        continue;
                    }
                    let ax = self.formation_x + i16::try_from(col).unwrap_or(0) * 2;
                    let ay = self.formation_y + i16::try_from(row).unwrap_or(0);
                    if bullet.x == ax && bullet.y == ay {
                        self.aliens[row][col] = false;
                        self.score = self
                            .score
                            .saturating_add(10 + i64::from((ALIEN_ROWS - row) as u16 * 4));
                        hit_alien = true;
                        break;
                    }
                }
                if hit_alien {
                    break;
                }
            }

            if !hit_alien {
                next_player.push(bullet);
            }
        }
        self.player_bullets = next_player;

        let mut next_enemy = Vec::new();
        let mut player_hit = false;
        let enemy_bullets = std::mem::take(&mut self.enemy_bullets);
        for bullet in enemy_bullets {
            if self.damage_shield(bullet.x, bullet.y) {
                continue;
            }

            if bullet.y == FIELD_H - 2 && bullet.x == self.player_x {
                player_hit = true;
                continue;
            }
            next_enemy.push(bullet);
        }
        self.enemy_bullets = next_enemy;

        if player_hit {
            self.lives = self.lives.saturating_sub(1);
            self.player_bullets.clear();
            self.enemy_bullets.clear();
            self.player_x = FIELD_W / 2;
            self.clear_active_powerups();
            if self.lives <= 0 {
                self.finished = true;
            }
        }
    }

    fn tick(&mut self, dt_ms: u32) {
        if self.finished {
            return;
        }

        self.player_cooldown_ms = self.player_cooldown_ms.saturating_sub(dt_ms);
        self.rapid_fire_ms = self.rapid_fire_ms.saturating_sub(dt_ms);

        self.move_accum_ms = self.move_accum_ms.saturating_add(dt_ms);
        loop {
            let interval = self.current_move_interval_ms();
            if self.move_accum_ms < interval {
                break;
            }
            self.move_accum_ms = self.move_accum_ms.saturating_sub(interval);
            self.move_formation();
        }

        self.maybe_spawn_ufo(dt_ms);
        self.step_ufo(dt_ms);
        self.maybe_enemy_shoot(dt_ms);
        self.step_power_up_drop(dt_ms);

        self.bullet_accum_ms = self.bullet_accum_ms.saturating_add(dt_ms);
        while self.bullet_accum_ms >= BULLET_STEP_MS {
            self.bullet_accum_ms = self.bullet_accum_ms.saturating_sub(BULLET_STEP_MS);
            self.step_bullets();
        }

        if let Some((_, _, max_y)) = self.alien_bounds()
            && max_y >= FIELD_H - 3
        {
            self.lives = 0;
            self.finished = true;
            return;
        }

        if self.live_aliens() == 0 {
            self.wave = self.wave.saturating_add(1);
            self.score = self.score.saturating_add(300);
            self.move_interval_ms = self.move_interval_ms.saturating_sub(30).max(110);
            self.reset_wave();
        }
    }

    fn handle_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Left | KeyCode::Char('a') => {
                self.player_x = self.player_x.saturating_sub(1).max(1);
            }
            KeyCode::Right | KeyCode::Char('d') => {
                self.player_x = self.player_x.saturating_add(1).min(FIELD_W - 2);
            }
            KeyCode::Char(' ') => {
                if self.player_cooldown_ms == 0
                    && self.player_bullets.len() < self.max_player_shots()
                {
                    self.player_bullets.push(Bullet {
                        x: self.player_x,
                        y: FIELD_H - 3,
                        dy: -1,
                    });
                    self.player_cooldown_ms = self.player_fire_cooldown_ms();
                }
            }
            _ => {}
        }
    }
}

impl Game for GalacticInvadersGame {
    fn id(&self) -> &'static str {
        crate::GALACTIC_INVADERS_ID
    }

    fn display_name(&self) -> &'static str {
        "Galactic Invaders"
    }

    fn init(&mut self, _ctx: &InitCtx) -> Result<()> {
        self.score = 0;
        self.lives = 3;
        self.wave = 1;
        self.finished = false;
        self.move_interval_ms = 420;
        self.next_ufo_power_up = PowerUpKind::RapidFire;
        self.clear_active_powerups();
        self.reset_wave();
        Ok(())
    }

    fn update(&mut self, event: RuntimeEvent, _ctx: &mut UpdateCtx) -> Result<()> {
        match event {
            RuntimeEvent::Input(key) => self.handle_input(key),
            RuntimeEvent::Tick { dt_ms } => self.tick(dt_ms),
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

        if frame.width < 50 || frame.height < 20 {
            let warning = "TERMINAL TOO SMALL FOR GALACTIC INVADERS";
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

        let buff_status = if self.rapid_fire_ms > 0 {
            let secs = self.rapid_fire_ms / 1000;
            let tenths = (self.rapid_fire_ms % 1000) / 100;
            format!("BUFF:RAPID({secs}.{tenths}s)")
        } else {
            "BUFF:NONE".to_string()
        };
        let status = format!(
            "GALACTIC INVADERS  SCORE:{}  LIVES:{}  WAVE:{}  {}",
            self.score, self.lives, self.wave, buff_status
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

        let ox = 4_i16;
        let oy = 2_i16;

        for x in 0..FIELD_W {
            frame.set(
                u16::try_from(ox + x).unwrap_or(0),
                u16::try_from(oy + FIELD_H - 1).unwrap_or(0),
                Cell {
                    glyph: '-',
                    fg: Color::DarkGray,
                    ..Cell::default()
                },
            );
        }

        for row in 0..ALIEN_ROWS {
            for col in 0..ALIEN_COLS {
                if !self.aliens[row][col] {
                    continue;
                }
                let x = ox + self.formation_x + i16::try_from(col).unwrap_or(0) * 2;
                let y = oy + self.formation_y + i16::try_from(row).unwrap_or(0);
                frame.set(
                    u16::try_from(x).unwrap_or(0),
                    u16::try_from(y).unwrap_or(0),
                    Cell {
                        glyph: 'W',
                        fg: Color::Green,
                        ..Cell::default()
                    },
                );
            }
        }

        for shield in &self.shields {
            frame.set(
                u16::try_from(ox + shield.x).unwrap_or(0),
                u16::try_from(oy + shield.y).unwrap_or(0),
                Cell {
                    glyph: '=',
                    fg: if shield.hp > 1 {
                        Color::Cyan
                    } else {
                        Color::Blue
                    },
                    ..Cell::default()
                },
            );
        }

        for bullet in &self.player_bullets {
            frame.set(
                u16::try_from(ox + bullet.x).unwrap_or(0),
                u16::try_from(oy + bullet.y).unwrap_or(0),
                Cell {
                    glyph: '|',
                    fg: Color::White,
                    ..Cell::default()
                },
            );
        }

        for bullet in &self.enemy_bullets {
            frame.set(
                u16::try_from(ox + bullet.x).unwrap_or(0),
                u16::try_from(oy + bullet.y).unwrap_or(0),
                Cell {
                    glyph: '!',
                    fg: Color::Red,
                    ..Cell::default()
                },
            );
        }

        if let Some(ufo) = self.ufo {
            frame.set(
                u16::try_from(ox + ufo.x).unwrap_or(0),
                u16::try_from(oy + 1).unwrap_or(0),
                Cell {
                    glyph: 'U',
                    fg: Color::Magenta,
                    ..Cell::default()
                },
            );
        }

        if let Some(drop) = self.power_up_drop {
            frame.set(
                u16::try_from(ox + drop.x).unwrap_or(0),
                u16::try_from(oy + drop.y).unwrap_or(0),
                Cell {
                    glyph: drop.kind.glyph(),
                    fg: drop.kind.color(),
                    ..Cell::default()
                },
            );
        }

        frame.set(
            u16::try_from(ox + self.player_x).unwrap_or(0),
            u16::try_from(oy + FIELD_H - 2).unwrap_or(0),
            Cell {
                glyph: '^',
                fg: Color::Yellow,
                ..Cell::default()
            },
        );

        if self.finished {
            let banner = "GAME OVER";
            let x = u16::try_from(ox + FIELD_W / 2 - 4).unwrap_or(0);
            let y = u16::try_from(oy + FIELD_H / 2).unwrap_or(0);
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
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Bullet, GalacticInvadersGame, PowerUpKind, RAPID_FIRE_DURATION_MS};
    use runtime::{Game, InitCtx, RuntimeEvent, UpdateCtx};

    fn init_game(seed: u64) -> GalacticInvadersGame {
        let mut game = GalacticInvadersGame::new(seed);
        game.init(&InitCtx {
            width: 100,
            height: 40,
            seed,
        })
        .expect("invaders init should succeed");
        game
    }

    fn space_key() -> KeyEvent {
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty())
    }

    fn frame_row_text(frame: &runtime::Frame, y: u16) -> String {
        let mut text = String::new();
        for x in 0..frame.width {
            if let Some(cell) = frame.get(x, y) {
                text.push(cell.glyph);
            }
        }
        text
    }

    #[test]
    fn player_shot_scores_when_hitting_alien() {
        let mut game = init_game(5);
        game.aliens = [[false; super::ALIEN_COLS]; super::ALIEN_ROWS];
        game.aliens[0][0] = true;
        game.formation_x = game.player_x;
        game.formation_y = 4;
        game.shields.clear();
        game.player_bullets.push(Bullet {
            x: game.formation_x,
            y: game.formation_y + 1,
            dy: -1,
        });
        game.step_bullets();

        assert!(game.score > 0);
        assert!(!game.aliens[0][0]);
    }

    #[test]
    fn wave_resets_after_clearing_aliens() {
        let mut game = init_game(8);
        game.aliens = [[false; super::ALIEN_COLS]; super::ALIEN_ROWS];
        let previous_wave = game.wave;

        let mut ctx = UpdateCtx::new(100, 40);
        game.update(RuntimeEvent::Tick { dt_ms: 16 }, &mut ctx)
            .expect("tick should succeed");

        assert_eq!(game.wave, previous_wave + 1);
        assert!(game.live_aliens() > 0);
    }

    #[test]
    fn deterministic_enemy_fire_for_same_seed() {
        let mut a = init_game(99);
        let mut b = init_game(99);
        let mut a_ctx = UpdateCtx::new(100, 40);
        let mut b_ctx = UpdateCtx::new(100, 40);

        for _ in 0..80 {
            a.update(RuntimeEvent::Tick { dt_ms: 16 }, &mut a_ctx)
                .expect("tick should succeed");
            b.update(RuntimeEvent::Tick { dt_ms: 16 }, &mut b_ctx)
                .expect("tick should succeed");
        }

        assert_eq!(a.enemy_bullets, b.enemy_bullets);
        assert_eq!(a.ufo, b.ufo);
        assert_eq!(a.score, b.score);
    }

    #[test]
    fn ufo_moves_without_player_bullets() {
        let mut game = init_game(13);
        game.ufo = Some(super::Ufo { x: 6, dir: 1 });
        game.ufo_move_accum_ms = 0;
        game.player_bullets.clear();

        game.tick(super::UFO_MOVE_INTERVAL_MS + 1);

        assert_eq!(game.ufo, Some(super::Ufo { x: 7, dir: 1 }));
    }

    #[test]
    fn enemy_fire_cadence_uses_real_dt() {
        let mut coarse = init_game(55);
        let mut fine = init_game(55);
        coarse.enemy_bullets.clear();
        fine.enemy_bullets.clear();
        coarse.enemy_shot_accum_ms = 0;
        fine.enemy_shot_accum_ms = 0;

        for _ in 0..20 {
            coarse.maybe_enemy_shoot(40);
        }
        for _ in 0..50 {
            fine.maybe_enemy_shoot(16);
        }

        assert_eq!(coarse.enemy_bullets, fine.enemy_bullets);
        assert_eq!(coarse.enemy_shot_accum_ms, fine.enemy_shot_accum_ms);
    }

    #[test]
    fn base_fire_model_limits_to_single_active_player_shot() {
        let mut game = init_game(17);
        game.player_cooldown_ms = 0;
        game.player_bullets.push(Bullet {
            x: game.player_x,
            y: super::FIELD_H - 4,
            dy: -1,
        });

        game.handle_input(space_key());

        assert_eq!(game.player_bullets.len(), 1);
    }

    #[test]
    fn rapid_fire_temporarily_allows_three_concurrent_shots_and_expires() {
        let mut game = init_game(21);
        game.rapid_fire_ms = RAPID_FIRE_DURATION_MS;
        game.player_bullets.clear();

        for _ in 0..4 {
            game.player_cooldown_ms = 0;
            game.handle_input(space_key());
        }
        assert_eq!(game.player_bullets.len(), 3);

        game.tick(RAPID_FIRE_DURATION_MS);
        assert_eq!(game.rapid_fire_ms, 0);
    }

    #[test]
    fn shield_repair_restores_bunkers_to_baseline() {
        let mut game = init_game(34);
        let baseline = game.shields.len();
        let target = game.shields[0];

        assert!(game.damage_shield(target.x, target.y));
        assert!(game.damage_shield(target.x, target.y));
        assert!(game.shields.len() < baseline);

        game.apply_power_up(PowerUpKind::ShieldRepair);

        assert_eq!(game.shields.len(), baseline);
    }

    #[test]
    fn player_hit_clears_active_powerups_and_pending_drop() {
        let mut game = init_game(89);
        game.rapid_fire_ms = 2000;
        game.power_up_drop = Some(super::PowerUpDrop {
            x: game.player_x,
            y: super::FIELD_H - 4,
            kind: PowerUpKind::RapidFire,
        });
        game.enemy_bullets.push(Bullet {
            x: game.player_x,
            y: super::FIELD_H - 3,
            dy: 1,
        });

        game.step_bullets();

        assert_eq!(game.rapid_fire_ms, 0);
        assert!(game.power_up_drop.is_none());
    }

    #[test]
    fn status_banner_displays_active_powerup_timer() {
        let mut game = init_game(1234);
        game.rapid_fire_ms = 5500;
        let mut frame = runtime::Frame::new(90, 34);

        game.render(&mut frame);
        let top_row = frame_row_text(&frame, 0);

        assert!(top_row.contains("BUFF:RAPID"));
    }
}
