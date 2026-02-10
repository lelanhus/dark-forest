# Releasing

Dark Forest releases from `main` using SemVer tags and a tag-driven GitHub Actions workflow.

## Release Model

- Trunk-based development.
- Milestone-based release readiness.
- Release only when DoD and quality gates are satisfied.

## Versioning Rules

- Follow Semantic Versioning.
- Breaking API or behavior changes increment major version.
- Backward-compatible features increment minor version.
- Fixes increment patch version.

## Artifact Contract (`v1.0.0`)

Tag `v1.0.0` must publish:

- `dark-forest-v1.0.0-linux-x86_64.tar.gz`
- `dark-forest-v1.0.0-linux-aarch64.tar.gz`
- `dark-forest-v1.0.0-macos-arm64.tar.gz`
- `dark-forest-v1.0.0-windows-x86_64.zip`
- `SHA256SUMS.txt`

Each packaged artifact includes:

- `dark-forest` binary (or `dark-forest.exe` on Windows)
- `LICENSE-APACHE`
- `LICENSE-MIT`
- `README.md`

## Trigger and Workflow

Release workflow file: `.github/workflows/release.yml`.

Triggers:

- `push` tag matching `v*`
- `workflow_dispatch` (manual run)

Publication step runs only for tag refs.

## Pre-Release Checklist

1. Milestone exit criteria met (`ROADMAP.md`).
2. Required local quality gates green:
   - `cargo fmt --all -- --check`
   - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
   - `cargo test --workspace --all-targets --all-features`
   - `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
3. Security-impact changes reviewed.
4. Changelog updated (`CHANGELOG.md`).
5. ADRs updated for key architecture/security/perf decisions.

## Cut Procedure

1. Ensure `main` contains the release commit.
2. Push `main`.
3. Create and push annotated tag:

```bash
git tag -a v1.0.0 -m "v1.0.0"
git push origin v1.0.0
```

1. Monitor `release` workflow run.
2. Verify GitHub Release assets and checksum file.

## Verification Checklist

- All four target artifacts are present.
- `SHA256SUMS.txt` is present and matches uploaded files.
- Replay smoke command succeeds on native-run targets in CI.
- Release notes and `CHANGELOG.md` entries match shipped behavior.

## Rollback Guidance

If a release has severe regression:

1. Stop forward merges for release branch activities.
2. Identify rollback target version.
3. Publish rollback advisory and mitigation.
4. Ship patched follow-up after root-cause fix and tests.
