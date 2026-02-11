# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

## [Unreleased]

### Added

- Galactic Invaders hardcore progression updates:
  - deterministic UFO power-up drops alternating `Rapid Fire` and `Shield Repair`
  - rapid-fire timed buff with status-banner timer display
  - shield-repair effect that restores bunker durability to baseline
- New Galactic Invaders unit coverage for:
  - UFO movement independent of player bullets
  - `dt_ms`-accurate enemy fire cadence
  - single-shot base-fire constraints
  - rapid-fire expiry behavior
  - power-up reset on player life loss
  - status banner power-up rendering

### Changed

- Runner startup now primes an initial game frame immediately after launch to avoid blank first-frame
  presentation in pane mode.
- Auto-fullscreen startup policy now applies to Galactic Invaders in addition to Tetris-like.
- Galactic Invaders gameplay pacing now scales using wave plus remaining-alien pressure for both
  formation movement and enemy fire cadence.
- Galactic Invaders controls summary now explicitly documents single-shot base fire and UFO power-up behavior.
- `tests/ptybox/actions/game_galactic_invaders.json` now asserts visible in-game markers
  (`GALACTIC INVADERS`, `SCORE:`, `WAVE:`) and rejects terminal-size warning fallback output during
  active play.
- `fixtures/replays/galactic-invaders-seed-777.json` now reflects tuned hardcore gameplay and
  produces a non-zero deterministic score baseline.

## [2.0.0] - 2026-02-10

### Added

- `crates/creator` crate for creator-tooling package/verification workflows.
- New creator CLI modes:
  - `--init-template <game_dir> [--id <game_id>] [--name <name>] [--author <author>] [--version <semver>]`
  - `--pack <game_dir> [--out <artifact.tar.gz>] [--metadata-out <metadata.json>]`
  - `--verify-artifact <artifact.tar.gz> [--metadata <metadata.json>]`
  - `--keygen <publisher_id> --out-dir <dir>`
  - `--sign-artifact <artifact> --publisher-id <id> --private-key <pk8>`
  - `--publish <artifact.tar.gz> --index <locator> [--metadata <metadata.json>] [--dry-run] [--replace]`
  - `--dev <game_dir> --index <locator> [--out <artifact.tar.gz>]`
    `[--metadata-out <metadata.json>] [--watch] [--interval-ms <ms>]`
    `[--dry-run] [--replace]`
- Publisher trust keyring management CLI:
  - `--publisher-key-list`
  - `--publisher-key-add <publisher_id> <public_key_base64>`
  - `--publisher-key-remove <publisher_id>`
- Keymap profile import/export CLI:
  - `--keymap-export <path>`
  - `--keymap-import <path>`
- `--reinstall <id>` content operation using persisted install provenance.
- Deterministic `.tar.gz` packaging flow with normalized archive headers and stable file ordering.
- Creator metadata sidecar JSON for packaged artifacts with signature/provenance fields:
  - `schema_version`
  - `game_id`
  - `version`
  - `entry_type`
  - `host_api`
  - `artifact_file`
  - `artifact_sha256`
  - `artifact_size_bytes`
  - `generated_at`
- `signature_file`
- `signature_sha256`
- `publisher_id`
- `publisher_key_fingerprint`
- Local index publish flow that stages artifact files near the target index and upserts catalog
  game/version entries.
- Publish controls for dry-run planning and explicit replacement of existing version artifacts when
  checksums differ.
- Creator dev loop API + CLI workflow that runs pack/verify/publish as one cycle or watches for
  local file changes and repeats automatically.
- Marketplace publish quality gates:
  - reject manifests that do not declare a `permissions` field
  - reject manifests whose `host_api` range is incompatible with host API `0.1.0`
- Install-time third-party verification:
  - required artifact checksum
  - detached signature fetch
  - trusted publisher lookup
  - Ed25519 signature verification
- Persisted install provenance fields (`artifact_uri`, checksum, publisher, signature fingerprint,
  verification timestamp).
- Starter creator template scaffold at `templates/wasm-basic`.
- Creator hot-reload documentation in `docs/CREATOR_WORKFLOW.md`.
- Template initialization workflow via creator CLI `--init-template`.
- `github://owner/repo@tag` registry provider support with release-asset catalog retrieval and
  cache fallback.
- Marketplace/listing metadata expansion:
  - verified status
  - publisher id
  - collections
  - compatibility badges
- New deterministic replay fixture `fixtures/replays/tetris-like-seed-4242.json`.
- Guideline-style Tetris-like gameplay systems:
  - 7-bag randomizer with all 7 tetrominoes
  - SRS rotation/wall-kick behavior (I and JLSTZ)
  - hold slot, ghost piece, and 5-piece next queue
  - hard drop / soft drop scoring and level-based gravity pacing
  - T-spin, combo, back-to-back, and perfect-clear scoring paths
- Tetris-like render fallback that prints an explicit terminal-size warning when the viewport is too small.
- Deterministic ptybox quality harness assets:
  - `scripts/ptybox_driver_assert.py` driver-first assertion runner with per-step diagnostics and
    `/tmp/df-ptybox-runs` artifacts.
  - `tests/ptybox/policy.cli.json` reusable CLI policy for local/offline ptybox execution.
  - `tests/ptybox/actions/*.json` reusable shell/game interactive driver scenarios.
  - `tests/ptybox/run_full_suite.sh` full CLI + replay + interactive matrix loop with 3-pass stability runs.
- Core built-in library expansion with two new native games:
  - Maze Chase (`maze-chase`) with pellets/power-pellets, frightened mode, and four ghost roles.
  - Galactic Invaders (`galactic-invaders`) with formation movement, shields, UFO bonuses, and wave progression.
- Deterministic replay fixtures:
  - `fixtures/replays/maze-chase-seed-9001.json`
  - `fixtures/replays/galactic-invaders-seed-777.json`
- Replay determinism coverage for Maze Chase and Galactic Invaders in `crates/app/tests/replay_determinism.rs`.
- Process-plugin launch warning confirmation overlay when process plugins are enabled.
- Error modal copy-to-clipboard action gated by settings and platform support.
- Keymap runtime precedence behavior:
  - per-game override > active profile > default profile
  - per-game overrides apply only in runner context
- Schema `3` backup/reinitialize policy for legacy on-disk roots.
- ADRs:
  - `docs/adr/0008-ed25519-signing-and-publisher-keyring.md`
  - `docs/adr/0009-github-registry-provider.md`

### Changed

- Workspace/package version baseline moved to `2.0.0`.
- Content/settings schema baseline moved to `3`.
- Creator metadata schema baseline moved to `2` with signature fields.
- Registry catalog/index contracts now use `artifact_sha256` and signature/publisher metadata.
- Interactive settings route now exposes keymap profile/security toggles plus diagnostics and
  registry visibility.
- Hot-load behavior for active games now uses explicit `Reload now`/`Later` prompt flow.
- Runner sizing now uses a single dimension helper across startup, resize, and fullscreen toggles.
- Starting `tetris-like` now auto-enables fullscreen runner mode so the 20-row board fits on common `80x24` terminals.
- Tetris-like input handling now uses deterministic held-key timing (`DAS`/`ARR`) and explicit key
  press/release event kinds instead of terminal autorepeat behavior.
- Tetris-like side panel now renders a stronger gameplay HUD hierarchy:
  - persistent HOLD card
  - labeled NEXT queue slots (`1`..`5`) with bordered cards and adaptive preview scaling up to 3x
  - stats column including `NEXT LVL` progress
- Tetris-like lock-resolution presentation now includes:
  - short line-clear flash before row collapse
  - transient event banners for line clears, T-spins, combo, back-to-back, and perfect clears
  - explicit paused overlay and boxed game-over alert
- Roadmap milestone `M7` status moved to complete for creator pack/verify/publish/dev plus
  template docs and marketplace quality gates.
- Shell command-palette route actions now require explicit confirmation before leaving an active runner session.
- Game listing metadata now supports optional `controls_summary` and Game Detail renders per-game controls.
- Shell filter/editor and detail metadata expanded for source/trust/compatibility context.
- Ptybox full-suite matrix now covers five built-ins and runner leave-confirm edge flow.

### Fixed

- `--install-index` no longer compares index artifact tarball checksums against unpacked-directory hashes.
  Install integrity remains enforced through registry-provider artifact checksum validation before unpack.
- Added regression coverage for publish/install roundtrips in app content-operation tests.
- Snake+ no longer self-collides on first tick after launch due to corrected initial body ordering.

## [1.0.0] - 2026-02-10

### Added

- Content transaction support for local install, rollback, verify, and remove operations.
- Installed metadata schema expansion with version history and per-version checksums.
- Permission grant persistence APIs (`load_permissions`/`save_permissions`) with typed capability grants.
- Settings-backed registry source configuration (`--registry-list`, `--registry-add`, `--registry-remove`).
- `index://` registry provider with local file and HTTP(S) fetch support.
- HTTP fetch retry/backoff with deterministic fallback-to-cache behavior for remote index catalogs.
- Tarball artifact unpack helper for index installs plus checksum validation.
- CLI content operation modes: `--install-local`, `--install-index`, `--update`, `--rollback`, `--verify`, `--remove`.
- Shell Installed action keybindings (`U` update, `B` rollback, `V` verify, `X` remove).
- Shell Library/Game Detail install keybinding (`I`) for non-installed marketplace entries.
- Asynchronous marketplace catalog refresh that merges remote entries into Library without blocking the UI loop.
- Wasmtime-backed third-party `entry_type=wasm` runtime integration.
- Manifest-driven launch resolver for installed entries with trust-policy enforcement.
- Capability enforcement contracts and default policy at the host boundary for WASM guest requests.
- Permission prompt flow (`Allow Once`, `Allow Always`, `Deny Once`, `Deny Always`) with remembered decisions.
- Settings route permission audit/revoke actions and CLI parity:
  - `--permissions-list [<game_id>]`
  - `--permissions-revoke <game_id> [--capability <cap>]`
- Install/update permission reconciliation that drops grants no longer declared by current manifest permissions.
- Security prompt policy controls in settings (`security_toggles.prompt_sensitive_only`).
- ADRs for:
  - content transactions and index provider
  - registry settings and shell marketplace integration
  - WASM runtime and capability enforcement
  - permission prompt/audit/revoke persistence semantics

### Changed

- Manifest parsing now normalizes mixed legacy/typed permission declarations into typed grants.
- Installed detail panel actions are now active commands rather than planned placeholders.
- App runtime now executes content operations through a serialized background worker queue.
- App interactive mode refreshes configured marketplace registries asynchronously and merges remote listings into Library.
- Third-party launch policy now allows `entry_type=wasm` and keeps third-party `native|process` disabled by default.
- Permission decisions now flow through session plus persisted grant checks before default policy handling.
- Remove operation revokes persisted permissions for the uninstalled game id.

### Fixed

- Hot-load refresh and operation completion now synchronize Installed route state without requiring app restart.
- Registry load failures surface as non-blocking warnings while continuing to load other configured registries.

## [0.1.0] - 2026-02-09

### Added

- Initial runtime/shell/game implementation for first release scope.
- Runner pause menu plus restart/quit confirmation overlays.
- Command palette commands for route navigation, launch, performance toggle, and help/diagnostics entrypoints.
- Contextual search overlay with query capture and filtering by id/name/description/tags.
- Game Detail stats summary (plays, best score, last played).
- Installed route metadata panel (version/source and planned actions).
- Content store support for loading/saving `installed.json` records with tests.
- Fixture-driven replay harness in `runtime` with deterministic outcome hashing.
- `dark-forest --replay <path>` headless mode for running replay fixtures.
- Runtime tests for auto-performance transitions, pause/resume dispatch, and panic isolation.
- Tag-driven release workflow producing Linux/macOS/Windows artifacts plus `SHA256SUMS.txt`.

### Changed

- App event pipeline now uses a bounded queue with controlled dropping for repeated non-critical navigation keys.
- Auto performance mode emits explicit `PerfModeChanged` signals when auto target shifts between 60 and 30 FPS.
- Runner rendering now honors target frame cadence (30/60/auto) independently from fixed tick simulation cadence.
- README and release docs now reflect shipped `v0.1.0` behavior and artifact contract.

### Fixed

- Corrected game start seeding so game instantiation and runtime init use the same deterministic seed.
- Runtime now isolates panics from game `update`/`render`, emits crash IDs, and returns control to
  the shell instead of terminating the host process.

### Security

- Private vulnerability reporting and coordinated disclosure policy documented.
