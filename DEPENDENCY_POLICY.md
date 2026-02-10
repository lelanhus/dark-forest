# Dependency Policy

Dark Forest uses an allowlist-plus-justification dependency model.

## Goals

- Minimize supply-chain risk.
- Keep dependency graph understandable.
- Prefer mature, maintained, well-audited crates.

## New Dependency Intake Checklist

Every new dependency PR must include:

1. Why this dependency is necessary.
2. Why existing dependencies or stdlib are insufficient.
3. Maintenance health summary (activity, issue response, release cadence).
4. Security posture summary (known advisories, unsafe usage footprint if relevant).
5. License compatibility confirmation with MIT/Apache-2.0 project licensing.
6. Expected long-term ownership/maintenance burden.

## Approval Rules

- New dependencies require maintainer approval.
- Dependencies with weak maintenance signals may be rejected.
- Dependencies with incompatible licensing are rejected.

## Version Management

- Prefer explicit semver constraints that avoid accidental major jumps.
- Keep lockfile updates scoped to intended changes.
- Review transitive changes in dependency update PRs.

## Security and License Monitoring

- Run advisory checks in CI.
- Run license checks in CI.
- Track high-risk dependencies for periodic review.
- Enforce dependency policy with:
  - `cargo deny check advisories licenses bans sources`

## Current Enforced Policy (`deny.toml`)

Licenses allowlisted in CI:

- `MIT`
- `Apache-2.0`
- `Apache-2.0 WITH LLVM-exception`
- `MPL-2.0`
- `BSD-3-Clause`
- `BSD-2-Clause`
- `ISC`
- `CDLA-Permissive-2.0`
- `Unicode-3.0`
- `Zlib`

Source restrictions in CI:

- Unknown registries are denied.
- Unknown git sources are denied.
- Allowed registry: crates.io index (`https://github.com/rust-lang/crates.io-index`).

## Deprecation and Removal

When a dependency is no longer justified:

1. Open a tracking issue.
2. Plan replacement or removal.
3. Remove with tests confirming behavior remains correct.
4. Update docs and changelog.
