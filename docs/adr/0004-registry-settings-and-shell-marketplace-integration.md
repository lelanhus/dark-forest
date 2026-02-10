# ADR-0004: Registry Settings and Shell Marketplace Integration

- Status: Accepted
- Date: 2026-02-10
- Deciders: Maintainers
- Supersedes: N/A
- Superseded by: N/A

## Context

M5 introduced `index://` provider support and CLI install/update paths, but interactive discovery
and install of remote content remained incomplete. The shell only displayed builtin and installed
entries, and registry sources were not persisted in settings.

To complete the M5 vertical slice, users need:

- persisted registry source configuration
- non-blocking remote catalog ingestion in interactive mode
- explicit install UX for remote listings without changing Enter-to-launch behavior

## Decision

Adopt the following implementation:

1. Extend `settings.json` with `registries: Vec<RegistryConfig>`, where each config contains:
   - `scheme` (currently `index`)
   - `locator`
2. Add CLI registry management commands:
   - `--registry-list`
   - `--registry-add <locator>`
   - `--registry-remove <locator>`
3. Run marketplace catalog refresh asynchronously in interactive mode (30s cadence) using
   configured index registries. Registry load failures are surfaced as non-modal notifications.
4. Merge marketplace listings into Library while preserving Installed filtering by installed ids.
5. Keep explicit install action via `I` on Library/Game Detail for non-installed entries;
   Enter remains launch-only.
6. Resolve duplicate marketplace ids by first configured registry; later duplicates are ignored
   and reported as warnings.

## Alternatives Considered

- Environment variable only registry source
  - Pros: minimal schema change
  - Cons: no persistent multi-registry UX
- New dedicated Marketplace route
  - Pros: isolated UI surface
  - Cons: more route complexity than needed for M5 completion
- Enter-to-install fallback for non-installed remote entries
  - Pros: fewer keybindings
  - Cons: ambiguous behavior and regression risk to launch flow

## Consequences

Positive:

- Interactive shell now supports remote discovery and installation from configured registries.
- Registry configuration is persisted and scriptable via deterministic CLI commands.
- UI responsiveness remains intact while remote listings refresh.

Negative:

- Additional background task and event wiring increase app-loop complexity.
- Registry warnings may surface repeatedly when source failures persist.

Operational impact:

- Settings schema now includes registry source records.
- Library contents become source-composed (builtin + marketplace + installed-only local ids).

## Validation

- Content tests cover settings round-trip and legacy settings compatibility without `registries`.
- App tests cover registry CLI parsing and behavior (list/add/remove) plus marketplace load/error
  handling and duplicate-id precedence.
- Shell tests cover `I` key install command emission and non-installed detail action text.
- Full local quality gate: `make ci`.

## Follow-up

- Add registry refresh backoff and warning coalescing controls.
- Expand provider support beyond `index` as new schemes are implemented.
- Add UI affordances for registry configuration in Settings route.
