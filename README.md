# Dark Forest

Dark Forest is a terminal-native arcade console written in Rust.

The mission is to deliver a terminal experience that feels like a GUI: fast, beautiful, extensible,
and safe by default, including on Raspberry Pi-class hardware.

## Status

Dark Forest is at first release scope (`v0.1.0`).

Implemented in `v0.1.0`:

- Shell routes: Home, Library, Installed, Settings, Game Detail, Runner.
- Global overlays: command palette (`Ctrl+K`), contextual search (`/`), help (`?`), notifications, progress, and error detail.
- Runtime contracts: fixed-timestep event loop, framebuffer model, diff rendering, pane/fullscreen runner, auto 30/60 policy.
- Built-in games: Snake+, Tetris-like, Micro Roguelite.
- Local persistence under `~/.dark-forest/` for settings, play history, installed records, and high scores.
- Replay harness and headless replay execution mode.

Implemented post-`v0.1.0` (current workspace):

- Content transaction pipeline for local installs (`tmp -> verify -> atomic move -> current pointer -> metadata commit`).
- Remove, rollback, and verify operations over installed versions.
- `index://` remote registry provider with local cache fallback.
- Remote artifact download, checksum validation, and tarball unpack support.
- Background operation queue from the shell for Installed actions (`U` update, `B` rollback, `V` verify, `X` remove).
- Polling-based hot-load refresh over `installed.json` and `games/**/game.json`.
- Manifest permission normalization from legacy strings to typed capability grants.

## Running

Interactive shell:

```bash
cargo run -p dark-forest
```

Headless replay mode:

```bash
cargo run -p dark-forest -- --replay fixtures/replays/snake-seed-12345.json
```

Content operations:

```bash
cargo run -p dark-forest -- --install-local /path/to/unpacked-game
cargo run -p dark-forest -- --install-index file:///path/to/index.json snake-plus --version 0.2.0
cargo run -p dark-forest -- --update snake-plus
cargo run -p dark-forest -- --rollback snake-plus
cargo run -p dark-forest -- --verify snake-plus
cargo run -p dark-forest -- --remove snake-plus
```

## Release Artifacts (v0.1.0)

Tag-driven release builds publish:

- `dark-forest-v0.1.0-linux-x86_64.tar.gz`
- `dark-forest-v0.1.0-linux-aarch64.tar.gz`
- `dark-forest-v0.1.0-macos-arm64.tar.gz`
- `dark-forest-v0.1.0-windows-x86_64.zip`
- `SHA256SUMS.txt`

## Architecture At A Glance

- `crates/shell`: routes, overlays, layout rendering, keymaps.
- `crates/runtime`: game contracts, event dispatch, timing, diff renderer, replay engine.
- `crates/games`: built-in native games.
- `crates/content`: JSON persistence, install transactions, rollback/verify, and permissions grant storage.
- `crates/registry`: builtin and `index://` providers, manifest normalization, artifact fetch/unpack helpers.
- `crates/plugin-host`: entry types and capability model contracts used by manifest normalization.
- `crates/theme`: Forge theme tokens and style helpers.
- `crates/diagnostics`: terminal and render diagnostics.

## Quality Standards

Required local quality gates:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

Additional policy and standards docs:

- `ENGINEERING_STANDARDS.md`
- `TESTING.md`
- `LINTING.md`
- `DEPENDENCY_POLICY.md`

## Contributing

See `CONTRIBUTING.md` for TDD-first workflow, Conventional Commits, and DCO requirements.

## Security

Do not disclose vulnerabilities in public issues. Use GitHub Security Advisories as defined in `SECURITY.md`.

## Roadmap

Milestone plan and release lifecycle: `ROADMAP.md`.

## License

Dual-licensed under either:

- Apache License 2.0 (`LICENSE-APACHE`)
- MIT (`LICENSE-MIT`)

at your option.
