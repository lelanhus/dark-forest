# ADR-0009: GitHub Registry Provider (`github://`)

- Status: Accepted
- Date: 2026-02-10
- Deciders: Maintainers
- Supersedes: N/A
- Superseded by: N/A

## Context

Milestone `M8` requires multi-source marketplace support beyond curated `index://`.
The short-term decision was to add a practical hosted source before OCI support.

Requirements:

- provider-level list/resolve support compatible with existing install flow
- predictable locator format
- resilient behavior under transient remote failures
- minimal operational complexity for creators

## Decision

1. Add `github://` provider in `crates/registry` with locator:
   - `github://owner/repo@tag`
2. Provider fetches GitHub release metadata and resolves a catalog asset (`index.json`) from the tagged release.
3. Catalog parsing and artifact resolution reuse registry index contracts used by `index://`.
4. Provider is selected via scheme-aware factory dispatch (`provider_from_locator`), shared by list/resolve/install flows.
5. Provider caches fetched release catalog payloads and uses cache fallback on remote failure.
6. OCI provider (`oci://`) is explicitly deferred from this milestone.

## Alternatives Considered

- Implement OCI first
  - Pros: stronger long-term artifact ecosystem alignment
  - Cons: higher implementation/ops complexity for immediate milestone scope
- Extend `index://` only
  - Pros: smallest short-term delta
  - Cons: does not satisfy multi-source/provider milestone objective
- Treat GitHub as plain file URL only
  - Pros: no provider abstraction changes
  - Cons: weak UX and no explicit source semantics

## Consequences

Positive:

- Practical multi-source marketplace path with low creator friction.
- Reuse of existing index contracts keeps install flow uniform.
- Scheme-aware dispatch scales to future providers.

Negative:

- Runtime dependency on GitHub API/release behavior for fresh data.
- Additional provider-specific cache and error handling paths.

Operational:

- Registry settings now may include both `index` and `github` schemes.
- Duplicate-id ordering remains first-registry-wins.
- Tests must cover rate-limit/failure/cache behaviors for provider reliability.

## Validation

- Provider unit tests for:
  - successful list/resolve from release asset
  - cache fallback when remote fetch fails
  - malformed catalog rejection
- App-level catalog aggregation tests with mixed registry sources.
- Full quality gates remain required before integration (`make ci`).
