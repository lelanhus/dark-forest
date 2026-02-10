# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

## [Unreleased]

### Added

- Content transaction support for local install, rollback, and verify operations.
- Installed metadata schema expansion with version history and per-version checksums.
- Permission grant persistence APIs (`load_permissions`/`save_permissions`) with typed capability grants.
- `index://` registry provider with cached index fallback and artifact download support.
- Tarball artifact unpack helper used by index installs.
- CLI operation modes: `--install-local`, `--install-index`, `--update`, `--rollback`, `--verify`, `--remove`.
- CLI registry management modes: `--registry-list`, `--registry-add`, `--registry-remove`.
- Shell Installed route action keybindings (`U` update, `B` rollback, `V` verify, `X` remove).
- Shell Library/Game Detail install keybinding (`I`) for non-installed marketplace entries.
- Polling hot-load detection for `installed.json` and `games/**/game.json`.
- New ADR for content transactions and index provider architecture.
- New ADR for registry settings and shell marketplace integration.

### Changed

- Manifest parsing now normalizes mixed legacy/typed permission declarations into typed grants.
- Installed detail panel actions are now active commands rather than planned placeholders.
- App runtime now executes content operations through a serialized background worker queue.
- App interactive mode now refreshes configured marketplace registries asynchronously and merges
  remote listings into Library.
- Remove operation revokes persisted permissions for the uninstalled game id.

### Fixed

- Hot-load refresh and operation completion now synchronize Installed route state without requiring app restart.

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
