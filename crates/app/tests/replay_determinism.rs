use std::path::PathBuf;

use anyhow::Result;
use runtime::{load_replay_from_path, run_replay};

#[test]
fn snake_replay_is_deterministic() -> Result<()> {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/replays/snake-seed-12345.json");
    let scenario = load_replay_from_path(&fixture_path)?;

    let run_once =
        || -> Result<runtime::ReplayOutcome> { run_replay(&scenario, games::instantiate) };

    let first = run_once()?;
    let second = run_once()?;

    assert_eq!(first.frame_hash, second.frame_hash);
    assert_eq!(first.score, second.score);
    assert_eq!(first.finished, second.finished);
    Ok(())
}

#[test]
fn tetris_replay_is_deterministic() -> Result<()> {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/replays/tetris-like-seed-4242.json");
    let scenario = load_replay_from_path(&fixture_path)?;

    let run_once =
        || -> Result<runtime::ReplayOutcome> { run_replay(&scenario, games::instantiate) };

    let first = run_once()?;
    let second = run_once()?;

    assert_eq!(first.frame_hash, second.frame_hash);
    assert_eq!(first.score, second.score);
    assert_eq!(first.finished, second.finished);
    Ok(())
}

#[test]
fn maze_chase_replay_is_deterministic() -> Result<()> {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/replays/maze-chase-seed-9001.json");
    let scenario = load_replay_from_path(&fixture_path)?;

    let run_once =
        || -> Result<runtime::ReplayOutcome> { run_replay(&scenario, games::instantiate) };

    let first = run_once()?;
    let second = run_once()?;

    assert_eq!(first.frame_hash, second.frame_hash);
    assert_eq!(first.score, second.score);
    assert_eq!(first.finished, second.finished);
    Ok(())
}

#[test]
fn galactic_invaders_replay_is_deterministic() -> Result<()> {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/replays/galactic-invaders-seed-777.json");
    let scenario = load_replay_from_path(&fixture_path)?;

    let run_once =
        || -> Result<runtime::ReplayOutcome> { run_replay(&scenario, games::instantiate) };

    let first = run_once()?;
    let second = run_once()?;

    assert_eq!(first.frame_hash, second.frame_hash);
    assert_eq!(first.score, second.score);
    assert_eq!(first.finished, second.finished);
    Ok(())
}
