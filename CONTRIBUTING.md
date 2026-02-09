# Contributing to Dark Forest

Thanks for contributing. This project is strict by design and optimized for long-term reliability.

## Before You Start

Read these documents first:

- `README.md`
- `ENGINEERING_STANDARDS.md`
- `TESTING.md`
- `LINTING.md`
- `DEPENDENCY_POLICY.md`
- `SECURITY.md`

## Development Prerequisites

- Rust stable toolchain (MSRV policy documented in `ENGINEERING_STANDARDS.md`)
- Git
- Node.js 20+ (for markdown linting)
- Python 3.12+ (for spell and YAML linting)
- Lychee (for link checks)
- Ability to run required lint/test/doc checks locally

## Branching and Release Model

- Trunk-based development.
- Base all work on `main`.
- Use short-lived branches.
- Release through SemVer tags from `main`.

## Commit Convention

Conventional Commits are required.

Examples:

- `feat(runtime): add fixed-step tick scheduler`
- `fix(content): preserve previous current pointer on failed verify`
- `docs(testing): clarify replay determinism requirements`

## Developer Certificate of Origin (DCO)

DCO sign-off is required on every commit.

Add this line to each commit message:

`Signed-off-by: Alex Contributor <alex.contrib@example.org>`

Example command:

`git commit -s -m "fix(runtime): prevent double-poll on input channel"`

PRs without DCO sign-off are blocked.

## Required TDD Workflow

1. Define acceptance criteria.
2. Add or update a failing test first.
3. Implement the minimal change to pass the test.
4. Refactor with tests green.
5. Add a regression test for bug fixes.
6. Document behavior changes.

## Local Quality Commands

Run these before opening a PR:

- `make ci-docs` for markdown, YAML, and spelling checks
- `make ci-rust` for Rust format/lint/test/doc/coverage/dependency checks
- `make ci` to run both

If the workspace has not been scaffolded yet (`Cargo.toml` missing), Rust checks are intentionally skipped.

## Pull Request Checklist

A PR is reviewable only if it includes:

- Problem statement and scope
- Linked issue or rationale
- Test-first evidence
- Lint/test/doc outputs
- Security impact note
- Dependency impact note
- ADR link when required
- DCO-compliant commits

Use `.github/PULL_REQUEST_TEMPLATE.md`.

## Review Expectations

- At least one maintainer review is required.
- Changes touching trust boundaries, permissions, or install/update logic require two maintainer reviews.
- Reviewers may reject changes that reduce clarity, determinism, or safety.

## Definition of Ready

A change is ready to start when:

- Acceptance criteria are explicit
- Scope is constrained
- A test strategy is defined
- Required interfaces and invariants are identified

## Definition of Done

A change is done when:

- All required checks pass
- Tests are sufficient and deterministic
- Docs and changelog are updated
- ADR is added/updated when required
- Review comments are resolved

## ADR Triggers

Add an ADR for changes that affect:

- Architecture boundaries
- Security model or capability enforcement
- Performance policy or runtime scheduling
- Data format compatibility
- `unsafe` usage exceptions
