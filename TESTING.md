# Testing Strategy

Dark Forest uses mandatory TDD and deterministic verification for critical paths.

## TDD Contract

For all behavior changes:

1. Red: write a failing test first.
2. Green: implement minimal code to pass.
3. Refactor: improve design while staying green.

No production change is accepted without test-first evidence.

## Test Layers

1. Unit tests

- Small, fast, isolated logic checks.
- Required for runtime primitives and data validation.

1. Integration tests

- Exercise crate boundaries and subsystem interactions.
- Required for install/update/rollback and registry/provider flows.

1. Snapshot tests

- Golden frame tests for renderer output.
- Must include terminal capability variance cases when relevant.

1. Replay tests

- Record input event streams and verify deterministic outcomes.
- Required for runtime pacing and gameplay consistency.

1. Property and fuzz tests

- Required for manifest parsing, registry payload parsing, and key trust-boundary parsers.

## Security and Reliability Test Expectations

- Permission enforcement tests must cover allow/deny/revoke cases.
- Transactional install tests must cover interruption and recovery behavior.
- Hot-load tests must verify no corrupted UI state.

## Coverage Policy

- Minimum 85% coverage for changed paths.
- CI currently runs a workspace-level 85% check as advisory during bootstrap until a changed-path coverage parser is introduced.
- PRs that reduce coverage quality require explicit maintainer rationale and follow-up issue linkage.
- Coverage is a gate, not a target ceiling.
- Exceptions require explicit maintainer approval in PR with rationale.

## Test Naming and Organization

- Name tests by behavior, not implementation detail.
- Keep test fixtures explicit and deterministic.
- Avoid flaky timing assumptions; use virtual/fixed clocks where possible.

## Bug Fix Requirements

Every bug fix must include:

1. A failing test reproducing the issue.
2. A passing fix.
3. A regression guard test.

## CI Expectations

Required checks include:

- Unit and integration tests
- Snapshot/replay suites (where applicable)
- Fuzz target smoke pass for relevant modules
- Coverage report and threshold validation
