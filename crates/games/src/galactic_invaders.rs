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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bullet {
    x: i16,
    y: i16,
    dy: i16,
    from_player: bool,
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
        self.player_x = FIELD_W / 2;
        self.player_cooldown_ms = 0;
        self.reset_shields();
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

    fn maybe_enemy_shoot(&mut self) {
        self.enemy_shot_accum_ms = self.enemy_shot_accum_ms.saturating_add(16);
        let cadence = 360_u32
            .saturating_sub(self.wave.saturating_sub(1).saturating_mul(20))
            .max(160);
        if self.enemy_shot_accum_ms < cadence {
            return;
        }
        self.enemy_shot_accum_ms = 0;

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
            from_player: false,
        });
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
            if let Some(mut ufo) = self.ufo {
                if bullet.y == 1 && bullet.x == ufo.x {
                    self.score = self.score.saturating_add(150);
                    self.ufo = None;
                    continue;
                }
                ufo.x += ufo.dir;
                if ufo.x <= 0 || ufo.x >= FIELD_W - 1 {
                    self.ufo = None;
                } else {
                    self.ufo = Some(ufo);
                }
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
        self.move_accum_ms = self.move_accum_ms.saturating_add(dt_ms);
        while self.move_accum_ms >= self.move_interval_ms {
            self.move_accum_ms = self.move_accum_ms.saturating_sub(self.move_interval_ms);
            self.move_formation();
        }

        self.maybe_spawn_ufo(dt_ms);
        self.maybe_enemy_shoot();

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
            self.move_interval_ms = self.move_interval_ms.saturating_sub(40).max(120);
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
                if self.player_cooldown_ms == 0 {
                    self.player_bullets.push(Bullet {
                        x: self.player_x,
                        y: FIELD_H - 3,
                        dy: -1,
                        from_player: true,
                    });
                    self.player_cooldown_ms = 170;
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

        let status = format!(
            "GALACTIC INVADERS  SCORE:{}  LIVES:{}  WAVE:{}",
            self.score, self.lives, self.wave
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
    use super::GalacticInvadersGame;
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

    #[test]
    fn player_shot_scores_when_hitting_alien() {
        let mut game = init_game(5);
        game.aliens = [[false; super::ALIEN_COLS]; super::ALIEN_ROWS];
        game.aliens[0][0] = true;
        game.formation_x = game.player_x;
        game.formation_y = 4;
        game.shields.clear();
        game.player_bullets.push(super::Bullet {
            x: game.formation_x,
            y: game.formation_y + 1,
            dy: -1,
            from_player: true,
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
}
