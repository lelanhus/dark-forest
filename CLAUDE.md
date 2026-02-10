# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Dark Forest is a terminal-native arcade console written in Rust (edition 2024, MSRV 1.93). It renders a GUI-like experience in the terminal using ratatui/crossterm, with a WASM sandbox for third-party games and a capability-based permission model.

## Build & Quality Commands

```bash
make ci              # Run all quality gates (docs + rust)
make ci-rust         # Format, clippy, tests, rustdoc, coverage, cargo-deny
make ci-docs         # Markdown lint, YAML lint, spelling, link check

# Individual Rust commands
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps

# Run the app
cargo run -p dark-forest
cargo run -p dark-forest -- --replay <path>    # Headless replay mode

# Run a single test
cargo test -p <crate-name> <test_name>

# Coverage (requires cargo-llvm-cov)
cargo llvm-cov --workspace --all-features --all-targets --summary-only
```

## Workspace Architecture

9-crate workspace with explicit trust boundaries:

```
crates/app/          Binary: main entry point, CLI args, event loop, operation dispatch
crates/shell/        UI routes (Home, Library, Settings, GameDetail, Runner),
                     overlays (CommandPalette, Search, Help, PermissionPrompt), keymaps
crates/runtime/      Game trait contracts, framebuffer, diff renderer, replay engine, perf modes
crates/content/      Install/update/rollback/verify transactions, atomic pointer switching,
                     permission persistence, settings persistence
crates/registry/     Provider abstraction (builtin://, index://), manifest parsing,
                     HTTP artifact fetch, tarball unpack
crates/plugin-host/  WASM execution (wasmtime), native dispatch, capability mediation,
                     permission prompt contracts
crates/games/        Built-in native games (Snake+, Tetris-like, Micro Roguelite)
crates/theme/        Theme tokens and style helpers
crates/diagnostics/  Terminal capability detection
```

**Key dependency flow**: app → shell, runtime, content, registry, plugin-host, games. Cross-crate calls go through explicit interfaces; don't leak internals across boundaries.

## Trust Model

- Built-in native games: **trusted**
- Third-party games: **sandboxed via WASM** by default
- Process plugins: **disabled** by default, require explicit opt-in
- Third-party `entry_type=native` is rejected unless explicitly whitelisted

## Concurrency Model

- UI loop never blocks on I/O
- Content operations serialized via dedicated worker queue
- Long-running ops (install, update, download) run in background tokio tasks with progress events
- Permission prompts queue from capability boundary without blocking the shell event loop

## Mandatory Practices

- **TDD**: Write failing test first, then implement minimal code to pass
- **No `unsafe`** in production paths (`unsafe_code = "deny"` workspace-wide). Exceptions require an ADR in `docs/adr/`
- **Conventional Commits** with DCO sign-off: `git commit -s -m "feat(runtime): ..."`
- **All quality gates must pass** before merging to main: `make ci`
- Coverage target: 85% on changed paths (advisory during bootstrap)

## Git Workflow

```bash
git switch -c codex/<topic>        # Branch from main
# ... implement with TDD ...
make ci                            # All gates green
git switch main && git merge --no-ff codex/<topic>
git push origin main
```

## Data & Persistence

- Storage root: `~/.dark-forest/` (platform-abstracted via `directories` crate)
- Schema version 2 in all persisted JSON documents
- Atomic `current` pointer updates for game installations (filesystem-atomic)
- Content failures preserve previous `current` pointer and quarantine partial staging

## Test Types

- **Unit**: isolated logic in each crate
- **Integration**: crate boundary and subsystem interactions
- **Snapshot**: golden frame tests for renderer output
- **Replay**: deterministic game execution verification (fixtures in `fixtures/replays/`)
- **Property/fuzz**: manifest and registry payload parsing (proptest in registry crate)

## Key Dependencies

| Dependency | Purpose |
|-----------|---------|
| ratatui 0.29 | TUI rendering (all-widgets) |
| crossterm 0.28 | Terminal I/O and events |
| tokio 1.44 | Async runtime (multi-thread) |
| wasmtime 41.0 | WASM sandbox execution |
| reqwest 0.12 | HTTP client (rustls-tls) |
| serde/serde_json | JSON serialization |
| sha2 | Artifact integrity verification |
| thiserror/anyhow | Typed and contextual error handling |

## Documentation Update Requirements

When changing behavior or policy, update the relevant docs:
- `SPEC.md` — product requirements
- `ARCHITECTURE.md` — structural design
- `DATA_MODEL.md` — persisted entity contracts
- `docs/adr/*` — architecture decisions (required for trust/security/perf/data changes)
- `CHANGELOG.md` — user-visible changes
