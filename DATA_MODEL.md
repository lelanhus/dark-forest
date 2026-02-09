# Data Model

This document defines canonical host-managed data contracts for Dark Forest.

Primary source: `SPEC.md`.

## Storage Root

Default host-managed data root:

- Unix-like systems: `~/.dark-forest/`
- Windows (planned): `%LOCALAPPDATA%/dark-forest/`

Paths must be resolved through a platform abstraction. Do not hardcode separators.

## Versioning and Compatibility

- Every persisted top-level document must include a `schema_version`.
- Schema upgrades must be forward-planned and backward-safe within a major release line.
- Current runtime schema baseline is `2` (installed-version history + typed permission grants).
- Breaking schema changes require:
  - ADR
  - migration plan
  - rollback plan
  - compatibility tests

## Core Entities

## GameListing

Purpose: catalog entry from builtin, local, or remote source.

Fields:

- `id: String` (stable slug, lowercase, `[a-z0-9._-]+`)
- `name: String`
- `description: String`
- `tags: Vec<String>`
- `author: String`
- `source: SourceRef`
- `permissions_summary: PermissionsSummary`
- `host_api_range: SemVerRange`
- `entry_type: EntryType` (`native|wasm|process`)

Invariants:

- `id` is immutable and globally unique within a registry namespace.
- `host_api_range` must parse and validate before listing is accepted.

## InstalledGame

Purpose: installation state for one game id.

Fields:

- `id: String`
- `installed_versions: Vec<SemVer>`
- `current_version: SemVer`
- `source_ref: SourceRef`
- `version_checksums: Map<SemVer, Sha256Hex>`

Invariants:

- `current_version` must exist in `installed_versions`.
- `installed_versions` must not contain duplicates.
- State must remain valid under interrupted installs.
- `version_checksums[current_version]` should exist for externally fetched artifacts when hash data is available.

## PlayHistory

Purpose: lightweight usage tracking.

Fields:

- `game_id: String`
- `last_played_at: Timestamp`
- `play_count: u64`
- `total_play_time_seconds: u64` (optional in early milestones)

Invariants:

- `play_count` is monotonic.
- `last_played_at` must not move backward unless repair/migration logic runs.

## HighScores

Purpose: per-game score table.

Fields:

- `game_id: String`
- `entries: Vec<ScoreEntry>`

`ScoreEntry`:

- `score: i64`
- `recorded_at: Timestamp`
- `metadata: Map<String, String>` (optional)

Invariants:

- Sort order policy is explicit per game (`higher_is_better` or `lower_is_better`).
- Max retained entries per game is bounded.

## Settings

Purpose: user and host configuration.

Fields:

- `theme: ThemeId`
- `performance_mode: PerformanceMode` (`auto|fps60|fps30`)
- `keymap_profile: KeymapProfileId`
- `registries: Vec<RegistryConfig>`
- `security_toggles: SecurityToggles`
- `diagnostics: DiagnosticsSettings`

Invariants:

- Unknown settings keys must be preserved or explicitly dropped by migration policy.
- Invalid enum values must fail validation and trigger safe fallback.

## PermissionsGrants

Purpose: capability grants per game.

Fields:

- `game_id: String`
- `grants: Vec<GrantRecord>`

`GrantRecord`:

- `capability: Capability`
- `scope: Scope`
- `decision: Decision` (`allow|deny`)
- `remembered: bool`
- `granted_at: Timestamp`

Invariants:

- Grants are evaluated least-privilege first.
- Deny rules override broad allow rules at equal specificity.

## Manifest Contract (`game.json`)

Normative fields:

- `id`
- `name`
- `version`
- `author`
- `entry_type`
- `entry`
- `host_api`
- `permissions`

Optional fields:

- `description`
- `controls`
- `tags`
- `homepage`
- `license`
- `changelog_url`

Validation rules:

- Unknown required fields are errors.
- Unknown optional extension fields are ignored unless policy says otherwise.
- Third-party native entry declarations must be rejected by policy unless explicitly trusted source rules allow them.
- `permissions` accepts legacy string labels and typed capability objects; host normalizes into typed grants.

## Integrity and Atomicity Requirements

- Install state writes must be crash-safe.
- `current` pointer updates must be atomic at filesystem level.
- Partial installs must not become visible as current.
- Recovery scan on startup must repair or quarantine invalid states.

## Migration Policy

- Migrations are additive whenever possible.
- Destructive transforms require backup snapshot first.
- Every migration includes:
  - id and source/target schema versions
  - deterministic transform
  - post-migration validation
  - rollback strategy
