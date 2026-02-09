use anyhow::Result;
use runtime::{RuntimeEvent, RuntimeRunner};

#[test]
fn runner_lifecycle_and_modes_work() -> Result<()> {
    let mut runner = RuntimeRunner::new(40, 20);
    let game = games::instantiate(games::SNAKE_ID, 42)?;

    runner.start(game, 42)?;
    assert!(runner.is_running());

    runner.toggle_pause();
    assert!(runner.is_paused());
    runner.toggle_pause();
    assert!(!runner.is_paused());

    runner.toggle_fullscreen();
    assert!(runner.is_fullscreen());

    runner.dispatch(RuntimeEvent::Tick { dt_ms: 120 })?;
    let delta = runner.render();
    assert!(!delta.changes.is_empty());

    runner.stop();
    assert!(!runner.is_running());
    Ok(())
}

#[test]
fn runner_handles_resize_without_panicking() -> Result<()> {
    let mut runner = RuntimeRunner::new(30, 12);
    let game = games::instantiate(games::TETRIS_ID, 7)?;
    runner.start(game, 7)?;

    runner.resize(80, 30);
    runner.dispatch(RuntimeEvent::Tick { dt_ms: 16 })?;
    let frame = runner.current_frame();

    assert_eq!(frame.width, 80);
    assert_eq!(frame.height, 30);
    Ok(())
}
