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
- Current runtime schema baseline is `3`.
- Breaking schema changes require:
  - ADR
  - migration plan
  - rollback plan
  - compatibility tests
- Legacy roots with mismatched schema are handled by backup + reinitialize:
  - existing root is renamed to a timestamped `*.schema-v<old>-backup-*` directory
  - fresh schema `3` seed files are created in a new root

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
- `artifact_uri: Option<String>`
- `checksum_sha256: Option<Sha256Hex>`
- `publisher_id: Option<String>`
- `signature_fingerprint: Option<String>`
- `verified_at: Option<Timestamp>`

Invariants:

- `current_version` must exist in `installed_versions`.
- `installed_versions` must not contain duplicates.
- State must remain valid under interrupted installs.
- `version_checksums[current_version]` should exist for externally fetched artifacts when hash data is available.
- For third-party installs, provenance fields must reflect the verified artifact and publisher key.

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
- `performance_mode: PerformanceMode` (`auto|60|30`)
- `keymap_active_profile: KeymapProfileId`
- `registries: Vec<RegistryConfig>`
- `security_toggles: SecurityToggles`
- `registry_filters: RegistryFilters`
- `allow_process_plugins: bool`
- `allow_error_clipboard_copy: bool`

Invariants:

- Unknown settings keys must be preserved or explicitly dropped by migration policy.
- Invalid enum values must fail validation and trigger safe fallback.
- `security_toggles.prompt_sensitive_only` defaults to `true` when missing.
- `allow_process_plugins` defaults to `false`.
- `allow_error_clipboard_copy` defaults to `false`.

## RegistryConfig

Purpose: persisted marketplace registry source configuration.

Fields:

- `scheme: String` (`index` or `github`)
- `locator: String` (provider locator accepted by `IndexRegistryProvider`, such as `file:///...`)

Invariants:

- Registry order is significant; first configured registry wins duplicate game ids.
- Empty locators are invalid for command-driven configuration.

## PublisherKeyring

Purpose: trusted publisher public keys used for signature verification.

Fields:

- `schema_version: u32`
- `keys: Map<String, PublisherKeyRecord>` keyed by `publisher_id`

`PublisherKeyRecord`:

- `publisher_id: String`
- `public_key_base64: String`
- `fingerprint_sha256: Sha256Hex`
- `added_at: Timestamp`

Invariants:

- `publisher_id` must be stable and unique in the keyring map.
- `fingerprint_sha256` must match the decoded public key bytes.

## KeymapProfiles

Purpose: user keybinding profiles and per-game runner overrides.

Fields:

- `schema_version: u32`
- `active_profile: String`
- `profiles: Map<String, KeymapProfile>`
- `game_overrides: Map<GameId, Map<ActionId, KeyBinding>>`

Invariants:

- `active_profile` should resolve to a key in `profiles`.
- Resolution precedence is:
  - per-game override (runner only)
  - active profile
  - default profile

## SecurityToggles

Purpose: persisted prompt policy controls for runtime capability decisions.

Fields:

- `prompt_sensitive_only: bool`

Invariants:

- Default value is `true` for backward-compatible settings loads.
- When `true`, first-use prompts are limited to sensitive capabilities (`fs.write`, `net`, `open_url`, `clipboard`).
- When `false`, host policy may prompt for any declared capability before grant.

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
- Creator `publish` requires the `permissions` field to be explicitly declared (array, may be empty).
- Creator `publish` requires `host_api` to parse as a semver range compatible with host API `0.1.0`.

## Registry Index Catalog (`index.json`)

Purpose: provider catalog contract consumed by `index://` and now authored by creator `publish`.

Top-level fields:

- `schema_version: u32` (current creator-emitted value: `2`)
- `games: Vec<IndexGame>`

`IndexGame` fields:

- `id: String`
- `name: String`
- `description: String` (optional, default empty)
- `tags: Vec<String>` (optional)
- `author: String`
- `permissions_summary: Vec<String>`
- `host_api_range: SemVerRange`
- `entry_type: EntryType` (optional override)
- `verified: bool`
- `publisher_id: Option<String>`
- `collections: Vec<String>`
- `compatibility: Option<CompatibilityBadge>`
- `versions: Vec<IndexVersion>`

`IndexVersion` fields:

- `version: SemVer`
- `artifact: String` (artifact locator relative to index location or absolute locator)
- `artifact_sha256: Sha256Hex` (optional, `checksum_sha256` accepted as legacy alias)
- `size_bytes: u64` (optional)
- `entry_type: EntryType` (optional override)
- `host_api: SemVerRange` (optional override)
- `permissions: Vec<PermissionDecl>` (legacy string or typed object)
- `signature_uri: Option<String>`
- `publisher_id: Option<String>`
- `signature_fingerprint: Option<String>`

Invariants:

- `host_api_range`/`host_api` values must parse as semver ranges.
- Creator-authored entries are accepted only when `host_api` is compatible with host API `0.1.0`.
- Signature fields are required for third-party marketplace publication/install flows.

## Creator Artifact Metadata (`*.metadata.json`)

Purpose: sidecar contract emitted by creator tooling for packaged artifacts.

Fields:

- `schema_version: u32`
- `game_id: String`
- `version: SemVer`
- `entry_type: EntryType`
- `host_api: SemVerRange`
- `artifact_file: String` (artifact filename, not absolute path)
- `artifact_sha256: Sha256Hex`
- `artifact_size_bytes: u64`
- `generated_at: Timestamp`
- `signature_alg: Option<String>` (`ed25519` for marketplace signing)
- `signature: Option<String>` (base64 detached signature payload)
- `publisher_id: Option<String>`
- `public_key_fingerprint: Option<String>`

Invariants:

- `schema_version` must match the creator metadata parser contract.
- `artifact_file` must match the verified artifact filename.
- `artifact_sha256` and `artifact_size_bytes` must match artifact bytes exactly.
- `game_id`, `version`, `entry_type`, and `host_api` must match unpacked `game.json`.
- Signed publication requires all signature fields and `signature_alg=ed25519`.

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
