# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog, and this project adheres to Semantic Versioning.

## [Unreleased]

## [2.0.0] - 2026-02-10

### Added

- Signature-aware creator workflows:
  - `--keygen <publisher_id> --out-dir <dir>`
  - `--sign-artifact <artifact> --publisher-id <id> --private-key <pk8>`
- Publisher trust keyring management CLI:
  - `--publisher-key-list`
  - `--publisher-key-add <publisher_id> <public_key_base64>`
  - `--publisher-key-remove <publisher_id>`
- Keymap profile import/export CLI:
  - `--keymap-export <path>`
  - `--keymap-import <path>`
- `--reinstall <id>` content operation using persisted install provenance.
- `github://owner/repo@tag` registry provider support with release-asset catalog retrieval and cache
  fallback.
- Install-time third-party verification:
  - required artifact checksum
  - detached signature fetch
  - trusted publisher lookup
  - Ed25519 signature verification
- Persisted install provenance fields (`artifact_uri`, checksum, publisher, signature fingerprint,
  verification timestamp).
- Marketplace/listing metadata expansion:
  - verified status
  - publisher id
  - collections
  - compatibility badges
- Shell filter/editor and detail metadata expansion for source/trust/compatibility context.
- Process-plugin launch warning confirmation overlay when process plugins are enabled.
- Error modal copy-to-clipboard action gated by settings and platform support.
- Keymap runtime precedence behavior:
  - per-game override > active profile > default profile
  - per-game overrides apply only in runner context
- Schema `3` backup/reinitialize policy for legacy on-disk roots.

### Changed

- Workspace/package version baseline moved to `2.0.0`.
- Content/settings schema baseline moved to `3`.
- Creator metadata schema baseline moved to `2` with signature fields.
- Registry catalog/index contracts now use `artifact_sha256` and signature/publisher metadata.
- Interactive settings route now exposes keymap profile/security toggles plus diagnostics and
  registry visibility.
- Hot-load behavior for active games now uses explicit `Reload now`/`Later` prompt flow.

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
