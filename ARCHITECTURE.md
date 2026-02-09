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
  - Install/update/remove/verify/rollback pipeline
  - Cache and local artifact management
  - Atomic pointer switching
- `crates/registry`
  - Registry provider traits
  - Listing/resolve flows
  - Provider adapters (`builtin://`, `local://`, later remote)
- `crates/plugin-host`
  - Execution type dispatch (`native|wasm|process`)
  - Capability mediation and grant checks
  - Sandbox integration boundary

## Trust Boundaries

- Built-in native games are trusted.
- Third-party games are sandboxed WASM by default.
- Process plugins are off by default and require explicit opt-in.

Policy gates:

- Third-party `entry_type=native` is rejected unless trust policy explicitly whitelists source.
- Capability checks are performed at host boundary, never delegated to plugin self-declaration.

## Async and Concurrency Model

- UI loop never performs blocking I/O directly.
- Long-running operations run in worker tasks with progress events.
- Shell receives operation state updates through message channels.
- Backpressure handling is explicit (bounded queues + drop/merge policy for non-critical telemetry).

## Failure Boundaries

- Shell failures should not corrupt content state.
- Content transaction failures must not leak partial installs.
- Registry/provider failures must degrade gracefully and keep local functionality available.
- Plugin failures are isolated from host process when possible.

## Data and Interface Contracts

- Runtime/game interface follows state-machine contract from `SPEC.md`.
- Manifest and persisted entities are defined in `DATA_MODEL.md`.
- Architecture/security/performance changes require ADR updates.

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
