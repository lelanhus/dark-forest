# Roadmap

Dark Forest uses milestone-based releases. Milestones ship when entry and exit criteria are satisfied, not by fixed dates.

Source of product scope: `SPEC.md`.

## Milestone Rules

- Each milestone has explicit entry criteria.
- Exit criteria must be testable and documented.
- Critical security or reliability regressions block release.
- Changes to milestone scope require maintainer approval.

## M0 - Foundation

Objective: establish repository, policies, documentation, and CI gate contracts.

Entry criteria:

- `SPEC.md` approved.
- Governance and contributor policies approved.

Exit criteria:

- Documentation baseline complete.
- Quality gates defined and enforceable.
- ADR process active.

Dependencies:

- None.

## M1 - Shell Skeleton

Objective: route and overlay skeleton with Forge theme rules.

Entry criteria:

- M0 complete.
- Architecture boundaries approved.

Exit criteria:

- Home/Library/Installed/Settings navigation functional.
- Global overlays available (`Ctrl+K`, `/`, `?`, notifications, progress drawer).
- No full-screen flicker under normal interaction.

Dependencies:

- M0.

## M2 - Runtime v0

Objective: stable runtime contracts and renderer behavior.

Entry criteria:

- M1 complete.
- Runtime interface ADR approved.

Exit criteria:

- Runtime state-machine contract implemented.
- Framebuffer and mandatory diff renderer in place.
- Pane/fullscreen modes functional.
- Auto 30/60 policy implemented with hysteresis.
- Minimal replay harness operational.

Dependencies:

- M1.

## M3 - Built-in Native Games (v0.1)

Objective: ship initial quality bar titles.

Entry criteria:

- M2 complete.

Exit criteria:

- `Snake+`, `Tetris-like`, and `Micro Roguelite` shipped.
- Shared pause/restart/quit UX across games.
- Local high scores and play stats implemented.
- Cross-platform binaries produced in CI.

Dependencies:

- M2.

## M4 - Local Installs and Hot-load (v0.2)

Objective: content management for local artifacts.

Entry criteria:

- M3 complete.
- Content install/rollback ADR approved.

Exit criteria:

- Local provider supports install/update/remove.
- Atomic `current` pointer flips verified.
- Hot-load watcher updates Library and Installed views.
- Rollback is immediate and reliable.

Dependencies:

- M2, M3.

## M5 - First Remote Registry Provider (v0.3)

Objective: remote source ingestion with cache/recovery behavior.

Entry criteria:

- M4 complete.
- Registry abstraction stable.

Exit criteria:

- At least one remote provider (`index://` or `github://`) implemented.
- Catalog/artifact caching and retry/recovery behavior documented and tested.
- Registry errors surfaced without UI lockup.

Dependencies:

- M4.

## M6 - Permission Enforcement and WASM Plugins (v1.0)

Objective: enforce trust boundaries for third-party games.

Entry criteria:

- M5 complete.
- Security enforcement ADRs approved.

Exit criteria:

- WASM third-party plugin execution supported.
- Capability grants enforced and revocable.
- Permission prompts and audit UI operational.
- At least one remote provider stable under policy gates.

Dependencies:

- M5.

## M7 - Creator Tooling (v1.1+)

Objective: accelerate ecosystem growth safely.

Entry criteria:

- M6 complete.

Exit criteria:

- `dev`, `pack`, `publish`, `verify` workflows defined and usable.
- Templates and hot-reload creator path documented.
- Marketplace quality gates enforce permissions and host API compatibility.

Dependencies:

- M6.

## M8 - Marketplace Maturity (v2.0+)

Objective: high-trust multi-source marketplace.

Entry criteria:

- M7 complete.

Exit criteria:

- Multi-registry support is stable.
- Verified publisher and signature workflows in place.
- Compatibility badges and collections operational.
- Reproducible install flow documented.

Dependencies:

- M7.

## M9 - Console OS Maturity (v3.0+)

Objective: long-horizon platform features.

Entry criteria:

- M8 complete.

Exit criteria:

- Profile support complete.
- Optional kid mode design finalized and shipped if accepted.
- Optional mods/replay-sharing/gamepad support assessed and shipped per policy.

Dependencies:

- M8.
