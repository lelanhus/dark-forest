# Governance

Dark Forest uses a maintainer-led model with a single final technical arbiter (BDFL).

## Roles

- BDFL/Maintainer Lead: final decision authority on technical disputes and roadmap priorities.
- Maintainers: review and merge changes, enforce standards, and steward architecture.
- Contributors: propose and implement changes following repository policy.

## Decision Classes

1. Routine decisions

- Normal bug fixes, docs updates, and scoped improvements.
- Resolved in PR review.

1. Significant technical decisions

- Architecture, security, performance, data compatibility, dependency policy, or unsafe exceptions.
- Must be recorded as ADRs in `docs/adr/`.

1. Emergency decisions

- Security incidents or release-blocking regressions.
- Maintainers may act immediately, then publish post-incident rationale.

## Decision Process

1. Problem statement and options are documented in issue/PR.
2. Maintainer discussion captures tradeoffs.
3. If decision is significant, ADR is required before merge.
4. BDFL resolves deadlocks.

## Escalation

- Step 1: Resolve in PR discussion.
- Step 2: Escalate to maintainers.
- Step 3: BDFL final ruling.

## Conflict Resolution

- Focus on technical evidence, reproducible facts, and project principles.
- Personal attacks or pressure-based decision making are out of bounds.
- Code of Conduct applies to all governance interactions.

## Maintainer Responsibilities

- Protect safety and reliability standards.
- Keep decisions documented and traceable.
- Keep contributor workflow clear and executable.
- Avoid silent policy drift.

## Governance Changes

Governance modifications require:

- A dedicated PR
- Maintainer review
- ADR update describing why governance changed
