# Roadmap

Dark Forest uses milestone-based releases. Milestones ship when entry and exit criteria are satisfied, not by fixed dates.

Source of product scope: `SPEC.md`.

## Milestone Rules

- Each milestone has explicit entry criteria.
- Exit criteria must be testable and documented.
- Critical security or reliability regressions block release.
- Changes to milestone scope require maintainer approval.

## Milestone Status Snapshot

- M0 Foundation: Complete
- M1 Shell Skeleton: Complete
- M2 Runtime v0: Complete
- M3 Built-in Native Games (`v0.1.0`): Complete
- M4 Local Installs + Hot-load (`v0.2`): In Progress
- M5 First Remote Provider (`v0.3`): In Progress
- M6+ : Pending

## M0 - Foundation (Complete)

Objective: establish repository, policies, documentation, and CI gate contracts.

Exit criteria met:

- Documentation baseline complete.
- Quality gates defined and enforceable.
- ADR process active.

## M1 - Shell Skeleton (Complete)

Objective: route and overlay skeleton with Forge theme rules.

Exit criteria met:

- Home/Library/Installed/Settings navigation functional.
- Global overlays available (`Ctrl+K`, `/`, `?`, notifications, progress, error detail).
- Keyboard-first flow stable with snapshot coverage.

## M2 - Runtime v0 (Complete)

Objective: stable runtime contracts and renderer behavior.

Exit criteria met:

- Runtime state-machine contract implemented.
- Framebuffer and diff renderer in place.
- Pane/fullscreen modes functional.
- Auto 30/60 policy implemented with hysteresis.
- Replay harness operational.

## M3 - Built-in Native Games (Complete, `v0.1.0`)

Objective: ship initial quality bar titles.

Exit criteria met:

- `Snake+`, `Tetris-like`, and `Micro Roguelite` shipped.
- Shared pause/restart/quit UX across games.
- Local high scores and play stats implemented.
- Cross-platform release artifact workflow defined for first release matrix.

## M4 - Local Installs and Hot-load (v0.2, In Progress)

Objective: content management for local artifacts.

Planned exit criteria:

- Local provider supports install/update/remove.
- Atomic `current` pointer flips verified.
- Hot-load watcher updates Library and Installed views.
- Rollback is immediate and reliable.

Current implementation slice:

- Local install transaction path with atomic `current` pointer file updates.
- Rollback and verify operations in content layer plus shell action triggers.
- Polling-based hot-load refresh for `installed.json` and `games/**/game.json`.

Dependencies:

- M2, M3.

## M5 - First Remote Registry Provider (v0.3, In Progress)

Objective: remote source ingestion with cache/recovery behavior.

Planned exit criteria:

- At least one remote provider (`index://` preferred) implemented.
- Catalog/artifact caching and retry/recovery behavior documented and tested.
- Registry errors surfaced without UI lockup.

Current implementation slice:

- `index://` provider implemented with list/resolve behavior.
- Remote index cache fallback on fetch failure.
- Artifact download + checksum verification + tarball unpack wiring.

Dependencies:

- M4.

## M6 - Permission Enforcement and WASM Plugins (v1.0, Pending)

Objective: enforce trust boundaries for third-party games.

Planned exit criteria:

- WASM third-party plugin execution supported.
- Capability grants enforced and revocable.
- Permission prompts and audit UI operational.
- At least one remote provider stable under policy gates.

Dependencies:

- M5.

## M7 - Creator Tooling (v1.1+, Pending)

Objective: accelerate ecosystem growth safely.

Planned exit criteria:

- `dev`, `pack`, `publish`, `verify` workflows defined and usable.
- Templates and hot-reload creator path documented.
- Marketplace quality gates enforce permissions and host API compatibility.

Dependencies:

- M6.

## M8 - Marketplace Maturity (v2.0+, Pending)

Objective: high-trust multi-source marketplace.

Planned exit criteria:

- Multi-registry support is stable.
- Verified publisher and signature workflows in place.
- Compatibility badges and collections operational.
- Reproducible install flow documented.

Dependencies:

- M7.

## M9 - Console OS Maturity (v3.0+, Pending)

Objective: long-horizon platform features.

Planned exit criteria:

- Profile support complete.
- Optional kid mode design finalized and shipped if accepted.
- Optional mods/replay-sharing/gamepad support assessed and shipped per policy.

Dependencies:

- M8.
