# Linting and Quality Gates

Dark Forest enforces strict linting and documentation quality gates.

## Mandatory Rust Gates

1. Formatting
- `cargo fmt --all -- --check`

2. Clippy
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Expected strict profile includes:
  - `clippy::all`
  - `clippy::pedantic`
  - `clippy::cargo`
  - Selected `clippy::nursery` lints when stable and useful

3. Rustdoc warnings as errors
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`

## Documentation Gates

- Markdown linting
- Broken link checks
- Spell/typo checks
- YAML linting for workflow and template files

Default command contract:

- `find . -type f -name '*.md' -not -path './.git/*' -print0 | xargs -0 markdownlint --config .markdownlint.yml`
- `yamllint -c .yamllint.yml .`
- `codespell --config .codespellrc`
- `lychee --config .lychee.toml './**/*.md' './**/*.yml' './**/*.yaml'`

## Dependency/Security/License Gates

- Vulnerability advisory checks must pass.
- License policy checks must pass.
- New dependency intake must satisfy `DEPENDENCY_POLICY.md`.

## Deny-by-Default Policy

- Warnings are treated as errors in CI.
- `#[allow(...)]` annotations require narrowly scoped justification comments.
- Broad crate-level lint suppression is disallowed unless approved by maintainer with ADR reference for high-impact cases.

## Exception Process

Exceptions are rare and must include:

1. Why the lint/gate is not currently satisfiable
2. Risk assessment
3. Bounded timeline for removal
4. Tracking issue link

Maintainer approval is required before merge.

## Branch Protection Contract

The `main` branch must require all mandatory quality checks before merge.

Required workflow statuses:

- `docs-quality`
- `policy-checks`
- `rust-quality`
