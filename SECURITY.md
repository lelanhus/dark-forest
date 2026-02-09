# Security Policy

## Supported Versions

Until the first tagged release, security support targets `main` only.

After releases begin, support windows will be listed here by version range.

## Reporting a Vulnerability

Report vulnerabilities privately through GitHub Security Advisories for this repository.

- Preferred channel: GitHub private advisory report
- Do not open public issues for undisclosed vulnerabilities

If you cannot use GitHub advisories, contact the maintainer directly via GitHub handle `@x24102` and request a private handoff.

## What to Include in a Report

- Affected component(s)
- Impact summary and threat model
- Reproduction steps or proof of concept
- Suggested mitigation (if available)
- Whether exploitation is known in the wild

## Response Targets

- Initial acknowledgment: within 2 business days
- Triage classification: within 5 business days
- Mitigation plan for confirmed issues: within 10 business days

Severity handling priority:

- Critical: immediate triage, fix as top priority
- High: fix in nearest patch cycle
- Medium/Low: scheduled based on risk and release plan

## Coordinated Disclosure

- The project follows coordinated disclosure.
- Public disclosure should happen after a fix is available or mitigation guidance is published.
- Credit is provided to reporters unless they request anonymity.

## Security Release and Communication

For confirmed vulnerabilities:

1. Prepare fix and regression tests.
2. Validate with strict lint/test gates.
3. Publish release notes with impact, affected versions, and mitigation.
4. Backport where feasible based on supported versions.

## Scope Notes

Security-sensitive areas include:

- Plugin execution boundaries (native/WASM/process)
- Capability permission enforcement
- Install/update/rollback integrity
- Registry and artifact verification
