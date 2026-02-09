# Dark Forest

Dark Forest is a terminal-native arcade console written in Rust.

The mission is to ship a terminal experience that feels like a GUI: fast, beautiful, extensible, and safe by default, including on Raspberry Pi-class hardware.

## Project Status

Pre-implementation planning baseline.

- Product specification is defined in `SPEC.md`.
- Engineering and contributor workflows are documented in this repository.
- Runtime and feature code has not started yet.

## Project Principles

- Safety before convenience.
- Deterministic behavior over hidden magic.
- Declarative design over imperative sprawl.
- Convention over configuration.
- No UI stalls on blocking I/O.
- No flicker in terminal rendering.

## Architecture At A Glance

Planned top-level areas:

- Shell UI (`home`, `library`, `installed`, `settings`).
- Runtime engine (input, timing, framebuffer, diff renderer).
- Content system (install, update, verify, rollback).
- Registry system (builtin, local, and remote providers).
- Plugin execution (trusted native, sandboxed WASM, guarded process mode).

Detailed architecture contract: `ARCHITECTURE.md`.

## Target Platforms

- Linux (`x86_64`, `aarch64`) with Pi-class devices as a priority.
- macOS.
- Windows.

## Quality Bar

All changes are expected to follow:

- Mandatory TDD workflow: see `TESTING.md`.
- Strict lint and docs checks: see `LINTING.md`.
- Adapted NASA-style mission-critical standards: see `ENGINEERING_STANDARDS.md`.
- Dependency allowlist and review gate: see `DEPENDENCY_POLICY.md`.

Local quality command:

- `make ci` (runs docs checks and Rust checks; Rust checks auto-skip until workspace exists)

## Contributing Quickstart

1. Read `CONTRIBUTING.md`, `ENGINEERING_STANDARDS.md`, and `TESTING.md`.
2. Create a short-lived branch from `main`.
3. Write a failing test that proves the required behavior.
4. Implement the minimum change needed to pass tests.
5. Run required local checks.
6. Open a PR with evidence and a `Signed-off-by` line (DCO).

## Security

Do not report vulnerabilities in public issues.

Use GitHub Security Advisories for private disclosure as defined in `SECURITY.md`.

## Roadmap

Milestone-based roadmap: `ROADMAP.md`.

## Governance

Project decision model and escalation path: `GOVERNANCE.md`.

## License

Dual-licensed under:

- Apache License 2.0 (`LICENSE-APACHE`)
- MIT License (`LICENSE-MIT`)

At your option.
