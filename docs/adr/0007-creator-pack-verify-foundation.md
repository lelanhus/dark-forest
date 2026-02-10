# ADR-0007: Creator Pack/Verify Foundation

- Status: Accepted
- Date: 2026-02-10
- Deciders: Maintainers
- Supersedes: N/A
- Superseded by: N/A

## Context

`v1.0.0` completed runtime/content/registry trust boundaries, but milestone `M7` requires creator
tooling (`dev`, `pack`, `publish`, `verify`). The first implementation slice needs:

- deterministic artifact packaging so creators and CI can produce reproducible outputs
- machine-readable package metadata for downstream automation
- deterministic artifact verification against metadata and packaged manifest
- a crate boundary that can evolve independently from `crates/app` shell concerns

Without an explicit creator boundary, packaging logic would be duplicated or tightly coupled to
registry/runtime code paths that solve different problems.

## Decision

1. Introduce `crates/creator` as the owner of creator-packaging/verification APIs.
2. Add CLI entrypoints in `crates/app`:
   - `--pack <game_dir> [--out <artifact.tar.gz>] [--metadata-out <metadata.json>]`
   - `--verify-artifact <artifact.tar.gz> [--metadata <metadata.json>]`
   - `--publish <artifact.tar.gz> --index <locator> [--metadata <metadata.json>]` (local index only)
3. `pack` produces:
   - deterministic `.tar.gz` artifact with stable lexical file ordering
   - normalized archive header fields for byte-repeatability
   - sidecar metadata JSON (`*.metadata.json`) including identity + checksum + size contract
4. `verify-artifact` enforces:
   - checksum and byte-size match against metadata
   - artifact filename match against metadata
   - unpacked `game.json` identity/compat fields match metadata (`id`, `version`, `entry_type`, `host_api`)
5. `publish` consumes verified artifacts + metadata, stages artifacts into the target local index
   directory, and upserts index catalog entries for game/version.
6. Keep scope CLI-only for this slice; no interactive shell creator routes in `M7` slice 1.

## Alternatives Considered

- Implement creator features directly in `crates/app`
  - Pros: minimal crate churn
  - Cons: app crate grows orchestration + packaging internals, harder to test/reuse
- Place creator logic in `crates/registry`
  - Pros: shared artifact format knowledge
  - Cons: conflates provider resolution concerns with creator-side build concerns
- Non-deterministic pack flow
  - Pros: simpler implementation
  - Cons: reproducibility regressions, weak CI validation surface

## Consequences

Positive:

- Creator API boundary is explicit and testable in isolation.
- Pack outputs are deterministic for identical input trees on the same platform.
- Verification has a stable contract suitable for CI and future publish workflows.

Negative:

- New metadata schema introduces another contract to maintain.
- Deterministic archive rules constrain future format changes (must remain backward compatible).

Operational impact:

- `README.md`, `ROADMAP.md`, and `DATA_MODEL.md` must track creator metadata + CLI behavior.
- Local publish flow reuses creator metadata instead of recomputing package facts.

## Validation

- Unit tests in `crates/creator` cover pack output, determinism, verify success/failure, and local
  index publish behavior.
- App CLI parsing tests cover `--pack`, `--verify-artifact`, and `--publish` command surfaces.
- Full local quality gates remain required (`make ci`).

## Follow-up

- Define signing model and verification extensions for marketplace trust requirements.
- Add `dev` hot-reload flow and templates to complete remaining `M7` exit criteria.
