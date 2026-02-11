mod galactic_invaders;
mod maze_chase;
mod micro_roguelite;
mod snake;
mod tetris_like;

use anyhow::Result;
use plugin_host::EntryType;
use registry::{GameListing, SourceRef};
use runtime::Game;

pub const SNAKE_ID: &str = "snake-plus";
pub const TETRIS_ID: &str = "tetris-like";
pub const ROGUELITE_ID: &str = "micro-roguelite";
pub const MAZE_CHASE_ID: &str = "maze-chase";
pub const GALACTIC_INVADERS_ID: &str = "galactic-invaders";

#[must_use]
pub fn builtin_catalog() -> Vec<GameListing> {
    vec![
        GameListing {
            id: SNAKE_ID.to_string(),
            name: "Snake+".to_string(),
            description: "Arcade loop with deterministic pacing.".to_string(),
            tags: vec!["arcade".to_string(), "builtin".to_string()],
            author: "Dark Forest".to_string(),
            source: SourceRef::builtin(),
            permissions_summary: vec!["terminal.raw_input".to_string()],
            controls_summary: vec![
                "Move: arrows or WASD".to_string(),
                "Collect food, avoid collisions".to_string(),
            ],
            host_api_range: "^0.1".to_string(),
            entry_type: EntryType::Native,
        },
        GameListing {
            id: TETRIS_ID.to_string(),
            name: "Tetris-like".to_string(),
            description: "Timing and input precision with side panels.".to_string(),
            tags: vec![
                "arcade".to_string(),
                "puzzle".to_string(),
                "builtin".to_string(),
            ],
            author: "Dark Forest".to_string(),
            source: SourceRef::builtin(),
            permissions_summary: vec!["terminal.raw_input".to_string()],
            controls_summary: vec![
                "Move: arrows or WASD".to_string(),
                "Rotate: Z/X or Up/W".to_string(),
                "Hold: C, Hard drop: Space".to_string(),
            ],
            host_api_range: "^0.1".to_string(),
            entry_type: EntryType::Native,
        },
        GameListing {
            id: ROGUELITE_ID.to_string(),
            name: "Micro Roguelite".to_string(),
            description: "Turn-based map, log, and stats panes.".to_string(),
            tags: vec![
                "roguelike".to_string(),
                "turn-based".to_string(),
                "builtin".to_string(),
            ],
            author: "Dark Forest".to_string(),
            source: SourceRef::builtin(),
            permissions_summary: vec!["terminal.raw_input".to_string()],
            controls_summary: vec![
                "Move/attack: arrows or WASD".to_string(),
                "Defeat monsters before they defeat you".to_string(),
            ],
            host_api_range: "^0.1".to_string(),
            entry_type: EntryType::Native,
        },
        GameListing {
            id: MAZE_CHASE_ID.to_string(),
            name: "Maze Chase".to_string(),
            description: "Arcade maze run with power pellets and ghost pressure.".to_string(),
            tags: vec![
                "arcade".to_string(),
                "maze".to_string(),
                "builtin".to_string(),
            ],
            author: "Dark Forest".to_string(),
            source: SourceRef::builtin(),
            permissions_summary: vec!["terminal.raw_input".to_string()],
            controls_summary: vec![
                "Move: arrows or WASD".to_string(),
                "Power pellets enable ghost captures".to_string(),
            ],
            host_api_range: "^0.1".to_string(),
            entry_type: EntryType::Native,
        },
        GameListing {
            id: GALACTIC_INVADERS_ID.to_string(),
            name: "Galactic Invaders".to_string(),
            description: "Formation shooter with shields, UFO bonuses, and escalating waves."
                .to_string(),
            tags: vec![
                "arcade".to_string(),
                "shooter".to_string(),
                "builtin".to_string(),
            ],
            author: "Dark Forest".to_string(),
            source: SourceRef::builtin(),
            permissions_summary: vec!["terminal.raw_input".to_string()],
            controls_summary: vec![
                "Move: Left/Right or A/D".to_string(),
                "Fire: Space".to_string(),
            ],
            host_api_range: "^0.1".to_string(),
            entry_type: EntryType::Native,
        },
    ]
}

pub fn instantiate(id: &str, seed: u64) -> Result<Box<dyn Game + Send>> {
    match id {
        SNAKE_ID => Ok(Box::new(snake::SnakeGame::new(seed))),
        TETRIS_ID => Ok(Box::new(tetris_like::TetrisLikeGame::new(seed))),
        ROGUELITE_ID => Ok(Box::new(micro_roguelite::MicroRoguelite::new(seed))),
        MAZE_CHASE_ID => Ok(Box::new(maze_chase::MazeChaseGame::new(seed))),
        GALACTIC_INVADERS_ID => Ok(Box::new(galactic_invaders::GalacticInvadersGame::new(seed))),
        _ => anyhow::bail!("unknown builtin game id: {id}"),
    }
}
