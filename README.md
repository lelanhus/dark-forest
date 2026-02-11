# Dark Forest

Dark Forest is a terminal-native arcade console written in Rust.

The mission is to deliver a terminal experience that feels like a GUI: fast, beautiful, extensible,
and safe by default, including on Raspberry Pi-class hardware.

## Status

Dark Forest is at `v1.0.0`.

Implemented in `v1.0.0`:

- Shell routes: Home, Library, Installed, Settings, Game Detail, Runner.
- Global overlays: command palette (`Ctrl+K`), contextual search (`/`), help (`?`), notifications, progress, and error detail.
- Runtime contracts: fixed-timestep event loop, framebuffer model, diff rendering, pane/fullscreen runner, auto 30/60 policy.
- Built-in games: Snake+, Guideline-like Tetris-like, Micro Roguelite, Maze Chase, Galactic Invaders.
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
- Settings-backed registry configuration (`--registry-list`, `--registry-add`, `--registry-remove`).
- Asynchronous marketplace catalog refresh in interactive mode from configured `index` registries.
- Explicit remote install action in Library/Game Detail via `I` for non-installed marketplace entries.
- HTTP/HTTPS `index://` catalog loading with retry and deterministic cache fallback.
- Manifest-driven launch resolver for installed third-party entries.
- Third-party `entry_type=wasm` runtime integration via Wasmtime.
- Host-boundary capability enforcement for WASM guest capability requests.
- Permission prompt flow (`Allow Once`, `Allow Always`, `Deny Once`, `Deny Always`) with remembered decisions.
- Permission audit/revoke in Settings and CLI parity (`--permissions-list`, `--permissions-revoke`).
- Install/update permission reconciliation that drops grants no longer declared by manifest.
- Creator tooling: template scaffold (`--init-template`), deterministic packaging (`--pack`),
  artifact verification (`--verify-artifact`), and local publish (`--publish`).
- Creator dev workflow (`--dev`) with watch-mode rebuild/publish loops and documented starter
  template path.
- Creator template scaffold command (`--init-template`) to create a new WASM game directory.
- Marketplace publish quality gates for declared permissions and host API compatibility.
- Core library expansion with two additional built-in arcade games:
  - Maze Chase (`maze-chase`)
  - Galactic Invaders (`galactic-invaders`)
- Runner leave confirmation overlay for active-game route changes initiated from command palette route actions.
- Snake+ launch pacing fix to prevent immediate self-collision on first tick.
- Tetris-like now uses a Guideline-style ruleset:
  - 7-bag randomizer with SRS rotation/wall kicks
  - hold (`C`), ghost piece, and 5-piece next queue
  - soft drop (`Down`/`S`), hard drop (`Space`), rotate CW (`Up`/`W`/`X`), rotate CCW (`Z`)
  - deterministic input repeat model for held movement/drop (DAS/ARR) independent of terminal autorepeat
  - labeled `NEXT` slots (`1`..`5`) with adaptive preview scaling and a `NEXT LVL` goal indicator
  - line-clear flash and transient scoring feedback banners (line clears, T-spins, combos, B2B, perfect clear)
  - line clear, T-spin, combo, back-to-back, and perfect-clear scoring
  - auto-fullscreen launch so the full 20-row board renders correctly on common terminals

## Running

Interactive shell:

```bash
cargo run -p dark-forest
```

Headless replay mode:

```bash
cargo run -p dark-forest -- --replay fixtures/replays/snake-seed-12345.json
cargo run -p dark-forest -- --replay fixtures/replays/tetris-like-seed-4242.json
cargo run -p dark-forest -- --replay fixtures/replays/maze-chase-seed-9001.json
cargo run -p dark-forest -- --replay fixtures/replays/galactic-invaders-seed-777.json
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

Registry configuration:

```bash
cargo run -p dark-forest -- --registry-list
cargo run -p dark-forest -- --registry-add file:///path/to/index.json
cargo run -p dark-forest -- --registry-remove file:///path/to/index.json
```

Permissions audit/revoke:

```bash
cargo run -p dark-forest -- --permissions-list
cargo run -p dark-forest -- --permissions-list remote-wasm
cargo run -p dark-forest -- --permissions-revoke remote-wasm
cargo run -p dark-forest -- --permissions-revoke remote-wasm --capability net
```

Creator tooling:

```bash
cargo run -p dark-forest -- --init-template /tmp/my-wasm-game --id my-wasm-game --name "My WASM Game" --author "Your Name" --version 0.1.0
cargo run -p dark-forest -- --pack /path/to/game-dir
cargo run -p dark-forest -- --pack /path/to/game-dir --out /tmp/sample-game-1.2.3.tar.gz --metadata-out /tmp/sample-game-1.2.3.metadata.json
cargo run -p dark-forest -- --verify-artifact /tmp/sample-game-1.2.3.tar.gz --metadata /tmp/sample-game-1.2.3.metadata.json
cargo run -p dark-forest -- --publish /tmp/sample-game-1.2.3.tar.gz --index file:///tmp/index.json --metadata /tmp/sample-game-1.2.3.metadata.json
cargo run -p dark-forest -- --publish /tmp/sample-game-1.2.3.tar.gz --index file:///tmp/index.json --dry-run
cargo run -p dark-forest -- --publish /tmp/sample-game-1.2.3.tar.gz --index file:///tmp/index.json --replace
cargo run -p dark-forest -- --dev /path/to/game-dir --index file:///tmp/index.json
cargo run -p dark-forest -- --dev /path/to/game-dir --index file:///tmp/index.json --watch --interval-ms 500
```

Creator template + hot-reload workflow guide:
`docs/CREATOR_WORKFLOW.md` (`templates/wasm-basic` starter scaffold).

## Release Artifacts (v1.0.0)

Tag-driven release builds publish:

- `dark-forest-v1.0.0-linux-x86_64.tar.gz`
- `dark-forest-v1.0.0-linux-aarch64.tar.gz`
- `dark-forest-v1.0.0-macos-arm64.tar.gz`
- `dark-forest-v1.0.0-windows-x86_64.zip`
- `SHA256SUMS.txt`

## Architecture At A Glance

- `crates/shell`: routes, overlays, layout rendering, keymaps.
- `crates/runtime`: game contracts, event dispatch, timing, diff renderer, replay engine.
- `crates/games`: built-in native games.
- `crates/content`: JSON persistence, install transactions, rollback/verify, and permissions grant storage.
- `crates/registry`: builtin and `index://` providers, manifest normalization, artifact fetch/unpack helpers.
- `crates/plugin-host`: entry type policy, Wasmtime runtime adapter, capability mediation, and prompt request contracts.
- `crates/theme`: Forge theme tokens and style helpers.
- `crates/diagnostics`: terminal and render diagnostics.
- `crates/creator`: deterministic creator packaging (`pack`), artifact verification
  (`verify-artifact`), local index publication (`publish`), creator dev loop (`dev`), and
  template scaffolding (`init-template`).

## Quality Standards

Required local quality gates:

```bash
make ci
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo deny check advisories licenses bans sources
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
