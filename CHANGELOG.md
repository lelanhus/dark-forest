# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

## [Unreleased]

### Added

- Initial documentation baseline for governance, security, standards, and roadmap.
- Runner pause menu plus restart/quit confirmation overlays.
- Game Detail stats summary (plays, best score, last played).
- Runtime tests for auto-performance transition signaling and pause/resume event dispatch.
- Fixture-driven replay harness in `runtime` with typed replay events and deterministic outcome hashing.
- Home route now follows `Continue`, `Featured`, `Recently Played`, and `Updates` sections with persisted continue-target routing.
- Installed route now renders version/source metadata and planned verify/rollback/update actions.
- Content store now supports loading/saving `installed.json` records with tests.
- Added `dark-forest --replay <path>` headless mode for running replay fixtures from the CLI.

### Changed

- App event pipeline now uses a bounded queue with controlled dropping for repeated non-critical navigation keys.
- Auto performance mode now emits explicit `PerfModeChanged` signals when auto target shifts between 60 and 30 FPS.
- Runner rendering now honors target frame cadence (30/60/auto) independently from fixed tick simulation cadence.

### Deprecated

- None.

### Removed

- None.

### Fixed

- Corrected game start seeding so game instantiation and runtime init use the same deterministic seed.

### Security

- Defined private vulnerability reporting and coordinated disclosure policy.
