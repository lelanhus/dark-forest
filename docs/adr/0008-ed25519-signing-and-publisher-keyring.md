# ADR-0008: Ed25519 Signing and Publisher Keyring

- Status: Accepted
- Date: 2026-02-10
- Deciders: Maintainers
- Supersedes: N/A
- Superseded by: N/A

## Context

Milestone `M8` requires verified third-party marketplace installs and reproducible provenance.
The previous model trusted remote catalog metadata plus optional checksums, which was not sufficient
for publisher authenticity.

We need a signing model that:

- is lightweight for creators
- has deterministic verification inputs
- works offline once trust material is installed
- fits CLI-first workflows

## Decision

1. Marketplace signing uses **Ed25519** detached signatures.
2. Creator tooling supports:
   - `--keygen` for publisher key generation
   - `--sign-artifact` for detached artifact-signature generation
3. Creator metadata schema is bumped to `2` and includes:
   - `signature_alg`
   - `signature`
   - `publisher_id`
   - `public_key_fingerprint`
4. Marketplace publication requires signed metadata (`signature_alg=ed25519`).
5. Third-party install verification requires:
   - artifact checksum (`artifact_sha256`)
   - detached signature locator (`signature_uri`)
   - publisher id (`publisher_id`)
   - trusted key lookup in local publisher keyring
6. Publisher trust root is local/offline (`publisher_keys.json` under content root), managed by CLI.
7. Install provenance is persisted in installed records:
   - artifact URI
   - checksum
   - publisher id
   - signature fingerprint
   - verification timestamp

## Alternatives Considered

- RSA signatures
  - Pros: widely familiar
  - Cons: larger keys/signatures and more complex key handling for this scope
- In-band signature-only in metadata without detached signature URI
  - Pros: fewer files
  - Cons: weaker provider interoperability and provenance clarity
- Online trust service for publisher keys
  - Pros: centralized revocation
  - Cons: online dependency conflicts with offline-first operation

## Consequences

Positive:

- Stronger authenticity guarantees for third-party installs.
- Explicit publisher trust management and auditable fingerprints.
- Deterministic reinstall and troubleshooting from persisted provenance.

Negative:

- Additional key management burden for creators/users.
- More required fields in registry/index contracts and install flow.

Operational:

- CLI surface expands for key management and signing.
- Registry/index validation now rejects incomplete signature metadata.
- Documentation and data model contracts must track schema `3` and metadata schema `2`.

## Validation

- Positive and negative signature/install tests:
  - valid signature path
  - tampered artifact/signature
  - unknown publisher key
  - fingerprint mismatch
- Creator and app CLI parsing/execution tests for keygen/sign/publisher-key commands.
- Full quality gates remain required before integration (`make ci`).
