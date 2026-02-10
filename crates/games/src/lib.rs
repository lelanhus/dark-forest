mod micro_roguelite;
mod snake;
mod tetris_like;

use anyhow::Result;
use plugin_host::EntryType;
use registry::{CompatibilityBadge, GameListing, SourceRef};
use runtime::Game;

pub const SNAKE_ID: &str = "snake-plus";
pub const TETRIS_ID: &str = "tetris-like";
pub const ROGUELITE_ID: &str = "micro-roguelite";

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
            host_api_range: "^0.1".to_string(),
            entry_type: EntryType::Native,
            verified: true,
            publisher_id: Some("dark-forest".to_string()),
            collections: vec!["Featured".to_string(), "Best on Pi".to_string()],
            compatibility: Some(CompatibilityBadge {
                host_api: "compatible".to_string(),
                permissions: "low-risk".to_string(),
            }),
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
            host_api_range: "^0.1".to_string(),
            entry_type: EntryType::Native,
            verified: true,
            publisher_id: Some("dark-forest".to_string()),
            collections: vec!["Featured".to_string(), "Arcade".to_string()],
            compatibility: Some(CompatibilityBadge {
                host_api: "compatible".to_string(),
                permissions: "low-risk".to_string(),
            }),
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
            host_api_range: "^0.1".to_string(),
            entry_type: EntryType::Native,
            verified: true,
            publisher_id: Some("dark-forest".to_string()),
            collections: vec!["Featured".to_string(), "Roguelike".to_string()],
            compatibility: Some(CompatibilityBadge {
                host_api: "compatible".to_string(),
                permissions: "low-risk".to_string(),
            }),
        },
    ]
}

pub fn instantiate(id: &str, seed: u64) -> Result<Box<dyn Game + Send>> {
    match id {
        SNAKE_ID => Ok(Box::new(snake::SnakeGame::new(seed))),
        TETRIS_ID => Ok(Box::new(tetris_like::TetrisLikeGame::new(seed))),
        ROGUELITE_ID => Ok(Box::new(micro_roguelite::MicroRoguelite::new(seed))),
        _ => anyhow::bail!("unknown builtin game id: {id}"),
    }
}
