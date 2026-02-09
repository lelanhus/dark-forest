# ADR-0001: Foundation Principles for Dark Forest

- Status: Accepted
- Date: 2026-02-09
- Deciders: Maintainers
- Supersedes: N/A
- Superseded by: N/A

## Context

Dark Forest begins as a high-reliability terminal game console with strict safety and quality
goals. Early project decisions must prevent process drift and trust-boundary regressions as
implementation starts.

## Decision

Dark Forest adopts the following foundation principles:

1. Rust workspace architecture with explicit crate boundaries (`shell`, `runtime`, `content`, `registry`, `plugin-host`).
2. Strict quality gates on every PR (format, lint, tests, docs checks, dependency/security/license checks).
3. Mandatory TDD for behavior changes.
4. Third-party plugin execution defaults to sandboxed WASM; process plugins are disabled by default.
5. Native execution is trusted only for built-in code or explicitly trusted source policy.
6. `unsafe` Rust is forbidden by default and requires ADR-backed exception.
7. Governance uses maintainer-led BDFL decision authority with ADR traceability for significant decisions.

## Alternatives Considered

- Start with relaxed process and tighten later
  - Pros: lower initial friction
  - Cons: high risk of policy debt and inconsistent quality
- Single-crate architecture with deferred modularization
  - Pros: simpler short-term setup
  - Cons: weak boundaries for security/runtime/content concerns

## Consequences

Positive:

- Clear contributor expectations before implementation begins
- Strong baseline for safety-critical and performance-sensitive work
- Better long-term maintainability and onboarding

Negative:

- Higher early process overhead
- Slower merge velocity for low-value changes

Operational impact:

- Contributors must provide stronger evidence in PRs
- Maintainers must actively enforce policy contracts

## Validation

- Documentation baseline exists and is internally consistent.
- PR templates and governance docs require test-first evidence and ADR linkage where needed.
- Security reporting path is private and explicit.

## Follow-up

- Add ADRs for runtime scheduling policy details.
- Add ADRs for manifest enforcement policy and trust-source classification.
- Add ADR for content transaction and crash-recovery design.
