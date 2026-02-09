# Engineering Standards

Dark Forest prioritizes reliability, clarity, and safety over short-term speed.

## Core Engineering Principles

- Declarative-first design.
- Explicit state transitions.
- Deterministic behavior where feasible.
- Minimize hidden coupling and side effects.
- Prefer simple, auditable control flow.

## Adapted Mission-Critical Profile (NASA-Inspired)

These standards are adapted from mission-critical engineering practices and applied pragmatically.

1. Bounded complexity
- Keep functions small and focused.
- Keep nesting and branching constrained.
- Refactor when logic becomes difficult to reason about.

2. Structured control flow
- Avoid recursion in mission-critical runtime loops unless justified by ADR.
- Avoid unbounded loops without clear termination or watchdog checks.

3. Explicit error handling
- Handle all recoverable errors.
- Use typed error domains and meaningful context.
- Never swallow errors silently.

4. Invariants and contracts
- Validate assumptions at module boundaries.
- Fail fast with clear diagnostics when invariants break.

5. Traceability
- Critical requirements must map to tests.
- Security/performance-sensitive changes require rationale in PR and, when applicable, ADR.

## Rust Toolchain Policy

- Use pinned stable Rust in CI.
- Maintain explicit MSRV policy.
- MSRV changes require changelog entry and maintainer approval.

## Unsafe Rust Policy

`unsafe` is forbidden by default in production paths.

Allowed only when all conditions are met:

- No safe alternative is viable.
- ADR documents reason, risk, and mitigations.
- Dedicated review by maintainers.
- Targeted tests and, where relevant, benchmarks validate assumptions.
- Unsafe block is minimal and heavily documented.

## Declarative Over Imperative Guidance

Prefer:

- Data-driven tables and enums over ad hoc branching
- Pure transformations over in-place mutation where practical
- Immutable snapshots for rendering diff decisions

Avoid:

- Hidden global mutable state
- Temporal coupling requiring undocumented call order
- Complex state mutations spread across modules

## Error Handling Standards

- Use explicit `Result` propagation for recoverable failures.
- Include enough context for triage.
- Distinguish user-facing vs internal errors.
- Never expose sensitive details in user-facing surfaces.

## Observability Standards

- Emit structured logs for lifecycle events and failures.
- Include stable event ids for critical paths.
- Ensure diagnostics can explain "what happened" and "what to do next".

## Documentation and ADR Requirements

Update docs whenever behavior or policy changes.

ADR is required for:

- Architecture boundary changes
- Security/trust model changes
- Performance policy changes
- Data compatibility changes
- Unsafe usage exceptions

## Definition of Engineering Completion

A change is engineering-complete only when:

- Tests prove behavior
- Lint and docs gates pass
- Risks are documented
- Follow-on maintenance impact is acceptable
