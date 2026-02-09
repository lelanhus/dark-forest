# ADR-0002: Canonical Host Data Root Uses ~/.dark-forest

- Status: Accepted
- Date: 2026-02-09
- Deciders: Maintainers
- Supersedes: N/A
- Superseded by: N/A

## Context

Earlier planning documents referenced `~/.tui-arcade/` as the host data root. The project brand and binary identity are `dark-forest`, and no released runtime has persisted user data yet.

## Decision

Use `~/.dark-forest/` as the canonical host-managed data root for initial implementation.

Platform mappings:

- Unix-like: `~/.dark-forest/`
- Windows: `%LOCALAPPDATA%/dark-forest/`

## Alternatives Considered

- Keep `~/.tui-arcade/`
  - Pros: no doc updates
  - Cons: naming drift from project identity and future migration burden
- Dual-root compatibility immediately
  - Pros: forward compatibility if both roots are used
  - Cons: unnecessary complexity pre-release

## Consequences

Positive:

- Consistent project naming across binary, docs, and runtime storage.
- No migration burden before first public alpha.

Negative:

- Requires immediate documentation updates.

Operational impact:

- Content store implementation targets `~/.dark-forest/`.

## Validation

- `SPEC.md`, `DATA_MODEL.md`, `ARCHITECTURE.md`, and `README.md` reference `~/.dark-forest/`.
- Content store defaults to `~/.dark-forest/`.

## Follow-up

- If pre-release users exist before v1.0, add explicit compatibility scan/migration ADR.
