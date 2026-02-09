## Summary

- What problem does this change solve?
- Why is this approach correct?

## Scope

- In scope:
- Out of scope:

## TDD Evidence (Required)

- Failing test added/updated first:
- Passing result after implementation:
- Regression guard added (for bug fixes):

## Quality Gate Evidence (Required)

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
- [ ] `cargo test --workspace --all-targets --all-features`
- [ ] Docs lint/link/spell checks
- [ ] Coverage policy met (85% changed-path policy; currently enforced as workspace-level CI gate) or approved exception
- [ ] Dependency/security/license checks

## Security and Trust Boundary Impact

- Does this change affect permissions, plugin execution, registry behavior, or install integrity?
- If yes, describe risks and mitigations.

## Dependency Changes

- New dependencies:
- Justification and review summary:
- License compatibility confirmation:

## ADR and Documentation

- ADR required? [ ] Yes [ ] No
- ADR link (if required):
- Docs updated (`README`, `ARCHITECTURE`, `DATA_MODEL`, `TESTING`, etc.):
- Changelog updated (if user-visible):

## DCO

- [ ] All commits include `Signed-off-by` (DCO)
