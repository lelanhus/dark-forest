# Architecture

This document defines the structural architecture and module boundaries for Dark Forest.

## Architecture Goals

- Keep UI responsive at all times.
- Keep trust boundaries explicit and enforceable.
- Keep content operations crash-safe and recoverable.
- Keep APIs stable and testable.

## Planned Workspace Layout

The project uses a Rust workspace with crate-oriented boundaries.

- `crates/shell`
  - Route system
  - Overlays (palette/search/help/toasts/progress/errors)
  - Theme/chrome rendering orchestration
- `crates/runtime`
  - Event loop contracts
  - Tick scheduling
  - Framebuffer model
  - Diff renderer
- `crates/content`
  - Install/update/verify/rollback transaction pipeline
  - Local artifact staging and checksum validation
  - Atomic pointer switching (`games/<id>/current`)
  - Permissions grant persistence
- `crates/registry`
  - Registry provider traits
  - Listing/resolve flows
  - Provider adapters (`builtin://`, `index://`)
  - Tarball artifact fetch + unpack helpers
- `crates/creator`
  - Creator-facing deterministic artifact packaging (`pack`)
  - Artifact metadata generation and serialization
  - Artifact verification against metadata + embedded manifest
  - Local index publication flow (`publish`) for creator artifacts
  - Marketplace publish quality gates (declared permissions + host API compatibility)
  - Creator dev-cycle orchestration (`dev`) for pack/verify/publish loops
- `crates/plugin-host`
  - Execution type dispatch (`native|wasm|process`)
  - Wasmtime adapter for `entry_type=wasm`
  - Capability mediation and grant checks
  - Permission prompt request contracts for app/shell integration
  - Sandbox integration boundary

## Trust Boundaries

- Built-in native games are trusted.
- Third-party games are sandboxed WASM by default.
- Process plugins are off by default and require explicit opt-in.

Policy gates:

- Third-party `entry_type=native` is rejected unless trust policy explicitly whitelists source.
- Capability checks are performed at host boundary, never delegated to plugin self-declaration.
- Third-party `entry_type=process` remains disabled by default policy.

## Async and Concurrency Model

- UI loop never performs blocking I/O directly.
- Long-running operations run in worker tasks with progress events.
- Shell receives operation state updates through message channels.
- Backpressure handling is explicit (bounded queues + drop/merge policy for non-critical telemetry).
- Content operations are serialized (single in-flight install/update/rollback/verify) via a dedicated worker queue.
- Hot-load refresh uses polling (1s cadence) over installed-state and manifest files.
- Marketplace catalog refresh runs in a background task (30s cadence) and posts non-blocking
  catalog/error events into the UI loop.
- Permission prompt requests are queued from the capability enforcement boundary and surfaced as shell overlays
  without blocking the shell event loop.
- Runner execution pauses only while a permission prompt is active and resumes immediately after decision capture.

## Failure Boundaries

- Shell failures should not corrupt content state.
- Content transaction failures must not leak partial installs.
- Content transaction failure path must preserve previous `current` pointer and quarantine partial staging.
- Registry/provider failures must degrade gracefully and keep local functionality available.
- Plugin failures are isolated from host process when possible.
- Capability denials return structured plugin errors and must not crash the host process.

## Data and Interface Contracts

- Runtime/game interface follows state-machine contract from `SPEC.md`.
- Manifest and persisted entities are defined in `DATA_MODEL.md`.
- Architecture/security/performance changes require ADR updates.
- Host-managed persistent data root is `~/.dark-forest/` (platform abstractions map to OS-specific paths).

## Observability Contract

- Structured logging for key lifecycle events.
- Error categories include actionable context and correlation ids when available.
- Diagnostics view exposes:
  - terminal capability detection
  - render timing indicators
  - install/update status and failures

## Quality and Safety Constraints

- No implicit global mutable state in mission-critical paths.
- Invariants are checked at module boundaries.
- Recoverable errors are typed and propagated explicitly.
- Panics in runtime/content paths are treated as defects and must be addressed before release.
