# AGENTS.md

This file defines the operating contract for maintainers, contributors, and coding agents working in Dark Forest.

## Scope

These rules apply to every change in this repository, including documentation-only changes and later implementation changes.

## Mandatory Workflow Order

1. Confirm requirement and acceptance criteria.
2. Identify affected interfaces and invariants.
3. Write or update a failing test first for behavior changes.
4. Implement the minimum change to pass tests.
5. Run formatting, linting, tests, and docs checks.
6. Update documentation and ADRs when architecture, security, performance, or policy changes.
7. Push a `codex/<topic>` branch and open a PR to `main`.
8. Merge only after all required checks are green.

## Non-Negotiable Rules

- No implementation without tests.
- No `unsafe` Rust in production paths unless an ADR-approved exception exists.
- No silent behavior changes without changelog and test impact notes.
- No bypass of required quality gates.
- No direct pushes to `main`; use PRs only.

## Git and PR Workflow

`main` is branch-protected and must remain PR-only.

1. Branch from `main` using `codex/<topic>`.
2. Commit with Conventional Commits and DCO sign-off.
3. Push branch and open PR to `main`.
4. Required checks must pass: `docs-quality`, `rust-quality`, `semantic-pr-title`, `dco-signoff`.
5. Merge when checks are green; required approving reviews are set to `0` for solo-maintainer flow.
6. Keep linear history; use squash or rebase merges only.

## Command Standards

Use deterministic commands and fail fast on errors.

Required command groups (once code exists):

- Format: `cargo fmt --all -- --check`
- Lint: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Docs: `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
- Tests: `cargo test --workspace --all-targets --all-features`
- Docs quality: markdown lint, link check, and spell/typo checks

Standard local entrypoints:

- `make ci-docs`
- `make ci-rust`
- `make ci`
- `gh pr create --base main --head codex/<topic> --title \"<type>: <summary>\" --body \"...\"`
- `gh pr merge --squash --delete-branch`

## Lint and Test Gate Expectations

A change is not ready for review unless all required checks pass locally and in CI.

Minimum gates:

- Formatting clean
- Clippy clean under strict profile
- Tests passing
- Coverage policy satisfied (changed-path policy; currently enforced via workspace-level 85% CI gate)
- Docs checks passing
- Dependency/security/license checks passing

## Architecture Boundaries

Planned workspace boundaries:

- `crates/shell`: application shell and routing
- `crates/runtime`: input/timing/framebuffer/renderer contracts
- `crates/content`: install/update/rollback/verification
- `crates/registry`: provider abstraction and resolution
- `crates/plugin-host`: native/WASM/process execution boundaries

Cross-boundary calls must go through explicit interfaces. Avoid leaking low-level details across crate boundaries.

## Editing Constraints

- Keep changes scoped and reversible.
- Prefer small, reviewable commits.
- Preserve existing behavior unless change request states otherwise.
- Do not commit generated artifacts unless policy explicitly requires them.

## Documentation Requirements

Update docs with every behavior or policy change:

- `SPEC.md` for product-level requirements
- `ARCHITECTURE.md` for structural design
- `DATA_MODEL.md` for persisted contract changes
- `docs/adr/*` for architecture/security/perf decisions
- `CHANGELOG.md` for user-visible changes
