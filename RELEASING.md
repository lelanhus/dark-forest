# Releasing

Dark Forest releases from `main` using SemVer tags.

## Release Model

- Trunk-based development.
- Milestone-based release readiness.
- Release only when DoD and quality gates are satisfied.

## Versioning Rules

- Follow Semantic Versioning.
- Breaking API or behavior changes increment major version.
- Backward-compatible features increment minor version.
- Fixes increment patch version.

## Pre-Release Checklist

1. Milestone exit criteria met (`ROADMAP.md`).
2. Required tests and quality gates green.
3. Security-impact changes reviewed.
4. Changelog updated (`CHANGELOG.md`).
5. ADRs updated for key architecture/security/perf decisions.

## Tagging and Notes

- Create annotated tag: `vX.Y.Z`.
- Release notes must include:
  - summary of changes
  - breaking changes (if any)
  - migration notes
  - known limitations

## Rollback Guidance

If a release has severe regression:

1. Stop forward merges for release branch activities.
2. Identify rollback target version.
3. Publish rollback advisory and mitigation.
4. Ship patched follow-up after root-cause fix and tests.

## Post-Release Activities

- Verify artifacts and checksums.
- Monitor issue reports for regressions.
- Schedule follow-up fixes for non-blocking issues.
