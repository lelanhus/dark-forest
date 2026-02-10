# ADR-0006: Permission Prompt, Audit, and Revoke UX

- Status: Accepted
- Date: 2026-02-10
- Deciders: Maintainers
- Supersedes: N/A
- Superseded by: N/A

## Context

`v1.0.0` requires practical, user-visible control over runtime capability grants. Before this ADR,
grant storage existed but users had no complete first-use prompt workflow, no consolidated audit UX,
and no deterministic revoke path across shell and CLI.

Required outcomes for `v1.0.0`:

- first-use sensitive capability prompts in interactive mode
- remembered decisions persisted in `permissions.json`
- immediate revoke flow from shell Settings and CLI
- deterministic machine-readable permission audit/revoke CLI output

## Decision

Adopt the following UX and persistence semantics:

1. Prompt surfaced as shell overlay with four explicit actions:
   - `Allow Once`
   - `Allow Always`
   - `Deny Once`
   - `Deny Always`
2. Prompt defaults apply to sensitive capabilities (`fs.write`, `net`, `open_url`, `clipboard`)
   when `settings.security_toggles.prompt_sensitive_only=true` (default).
3. Session decisions (`Allow/Deny Once`) live in-memory only.
4. Remembered decisions (`Allow/Deny Always`) persist as capability grants per game in
   `permissions.json` and are reused on subsequent requests.
5. Settings route includes permission audit list and revoke actions (`Enter`/`X`) per selected entry.
6. CLI parity:
   - `--permissions-list [<game_id>]`
   - `--permissions-revoke <game_id> [--capability <cap>]`
7. Install/update reconciliation removes stale remembered grants that are no longer declared
   (or no longer scope-compatible) in current manifest permissions.

## Alternatives Considered

- No prompt flow; default deny sensitive capabilities permanently
  - Pros: simpler state machine
  - Cons: poor UX and no path for trusted user approval
- Prompt every capability request with no remembered option
  - Pros: strongest explicit consent posture
  - Cons: excessive friction and gameplay disruption
- CLI-only permission management
  - Pros: minimal shell UI changes
  - Cons: inaccessible for keyboard-only interactive users expecting in-app controls

## Consequences

Positive:

- Users can make explicit, reversible trust decisions without leaving interactive flow.
- Decision state is inspectable and scriptable through deterministic CLI output.
- Revoke behavior is immediate and takes effect on next capability request.

Negative:

- Additional shell overlay/event states increase UI-loop complexity.
- Prompt queue management is required to avoid duplicate prompt spam.

Operational impact:

- `permissions.json` becomes authoritative for remembered decisions.
- Permission audit entries are rendered in Settings from merged installed/permission state.

## Validation

- Shell tests cover prompt overlay decision commands and Settings revoke emission.
- App tests cover permission CLI parsing/behavior and deterministic list ordering.
- App runtime tests cover prompt decision plumbing and resume semantics.
- Content tests cover grant reconciliation on install/update.
- Full local quality gate: `make ci`.

## Follow-up

- Add bulk revoke actions and scoped revoke UX from dedicated permissions view.
- Add prompt cooldown/coalescing controls for repeated denied requests.
- Add export/import tooling for permission profiles across devices.
