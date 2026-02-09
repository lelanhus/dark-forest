# ADR-0003: Content Transactions and `index://` Remote Provider

- Status: Accepted
- Date: 2026-02-09
- Deciders: Maintainers
- Supersedes: N/A
- Superseded by: N/A

## Context

Dark Forest reached `v0.1.0` with builtin games and runtime-shell stability but lacked release-critical
content operations from roadmap milestones M4 and M5:

- Atomic local install/update/rollback transactions
- Reliable verify flow
- First remote provider with cache/recovery behavior

Without these contracts, progress toward v1 trust and marketplace readiness is blocked.

## Decision

Adopt a vertical-slice implementation for M4+M5 with the following architecture:

1. Content operations use a staged transaction path:
`tmp -> manifest check -> checksum check -> atomic version move -> atomic current pointer write -> installed metadata commit`.
2. Rollback repoints `games/<id>/current` to a prior installed version and updates installed metadata.
3. Verify recomputes the current version directory hash and compares against stored checksum when available.
4. First remote provider is `index://`:
   - list/resolve from a JSON index
   - cache index responses locally
   - fall back to cache on fetch failure
   - fetch tarball artifacts with checksum validation
5. Interactive shell operations are executed through a serialized background queue to prevent UI blocking.
6. Hot-load refresh uses polling (1 second cadence) for `installed.json` and `games/**/game.json`.

## Alternatives Considered

- Skip transactions and rely on direct copies
  - Pros: less code
  - Cons: high corruption risk on interruption
- Implement `github://` as first provider
  - Pros: practical source for real content
  - Cons: larger API/error complexity than needed for first remote abstraction
- File event watcher (`notify`) first
  - Pros: lower latency
  - Cons: platform variance and failure modes increase initial risk

## Consequences

Positive:

- M4 and M5 gain concrete, testable implementation momentum.
- Content operations become crash-resilient and auditable.
- Remote provider abstraction is validated with cache and integrity constraints.

Negative:

- Additional dependencies for HTTP/tar/checksum handling.
- Local operation semantics currently prioritize correctness over maximal throughput.

Operational impact:

- Install metadata schema advances to include version history and checksums.
- Operation queue introduces bounded capacity behavior in interactive mode.

## Validation

- Unit tests cover install atomicity, rollback, verify mismatch, legacy metadata migration, and permission grant round-trip.
- Registry tests cover index listing/resolution, cache fallback, checksum mismatch, and tarball unpack behavior.
- App tests cover CLI parsing, hot-load signature changes, and rollback operation effects.

## Follow-up

- Add remove/uninstall transaction support and recovery audits.
- Extend update support beyond `index://` sources as additional providers mature.
- Implement full M6 permission prompt/revoke UX and WASM execution enforcement on top of normalized permission data.
