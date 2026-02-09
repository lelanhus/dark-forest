use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use runtime::{RuntimeEvent, RuntimeRunner};

fn frame_to_string(runner: &RuntimeRunner) -> String {
    let frame = runner.current_frame();
    let mut out = String::new();

    for y in 0..frame.height {
        for x in 0..frame.width {
            out.push(frame.get(x, y).map_or(' ', |cell| cell.glyph));
        }
        out.push('\n');
    }

    out
}

#[test]
fn snake_replay_is_deterministic() -> Result<()> {
    let run_once = || -> Result<String> {
        let mut runner = RuntimeRunner::new(32, 20);
        let game = games::instantiate(games::SNAKE_ID, 12345)?;
        runner.start(game, 12345)?;

        let events = [
            RuntimeEvent::Input(KeyEvent::new(KeyCode::Right, KeyModifiers::empty())),
            RuntimeEvent::Tick { dt_ms: 120 },
            RuntimeEvent::Input(KeyEvent::new(KeyCode::Down, KeyModifiers::empty())),
            RuntimeEvent::Tick { dt_ms: 120 },
            RuntimeEvent::Input(KeyEvent::new(KeyCode::Left, KeyModifiers::empty())),
            RuntimeEvent::Tick { dt_ms: 120 },
        ];

        for event in events {
            runner.dispatch(event)?;
            let _ = runner.render();
        }

        Ok(frame_to_string(&runner))
    };

    let first = run_once()?;
    let second = run_once()?;

    assert_eq!(first, second);
    Ok(())
}
