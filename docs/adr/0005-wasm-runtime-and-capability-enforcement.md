# ADR-0005: WASM Runtime and Capability Enforcement

- Status: Accepted
- Date: 2026-02-10
- Deciders: Maintainers
- Supersedes: N/A
- Superseded by: N/A

## Context

`v1.0.0` requires runnable third-party games while preserving strict host trust boundaries.
Earlier milestones normalized manifest capability declarations, but runtime launch and host-call
enforcement for third-party content were still incomplete.

Required outcomes for `v1.0.0`:

- third-party execution path that is sandbox-friendly
- manifest-driven launch resolution from installed artifacts
- capability checks enforced at host boundary (not guest self-attestation)
- non-crashing failure behavior for denied capabilities and runtime traps

## Decision

Adopt the following runtime architecture:

1. Use Wasmtime as the host engine for third-party `entry_type=wasm` games.
2. Add `WasmGameAdapter` in `plugin-host` implementing the `runtime::Game` contract via a v1 ABI:
   - required exports: `df_init`, `df_update`, `df_cell`
   - optional exports: `df_score`, `df_finished`
3. Resolve installed game launch from current manifest in app:
   - builtins continue native launch path
   - third-party `entry_type=wasm` launches through `WasmGameAdapter`
   - third-party `entry_type=native|process` remains blocked by policy in `v1.0.0`
4. Add capability enforcement contracts in `plugin-host`:
   - `CapabilityRequest`
   - `CapabilityDecision`
   - `CapabilityEnforcer`
   - `PermissionPromptRequest`
5. Enforce capability requests in host import boundary with this order:
   - manifest declaration check
   - enforcer decision (session/persisted/default policy/prompt bridge)
   - deny returns structured guest-visible error code without host crash

## Alternatives Considered

- Native dynamic library plugins for third-party content
  - Pros: simple host call model
  - Cons: breaks sandbox goal and significantly raises trust risk
- Process plugin model for `v1.0.0`
  - Pros: stronger OS isolation potential
  - Cons: larger IPC/protocol surface and operational complexity for v1 scope
- Delay runtime integration to post-`v1.0.0`
  - Pros: less immediate implementation load
  - Cons: fails `v1.0` DoD requiring third-party WASM execution

## Consequences

Positive:

- `v1.0.0` ships runnable third-party WASM games with explicit host policy controls.
- Capability checks are enforced centrally and consistently at host boundary.
- Manifest-driven launch path eliminates builtin-only assumptions in installed flow.

Negative:

- Adds runtime dependency and build-time footprint from Wasmtime.
- Introduces more policy and error-path complexity in app/plugin-host integration.

Operational impact:

- Runtime contract is documented in `docs/WASM_RUNTIME_CONTRACT.md`.
- Third-party manifest validation and entry-type policy become launch-critical gates.

## Validation

- Plugin-host tests verify WASM adapter render/update smoke behavior.
- Plugin-host tests verify undeclared capability denial and declared capability handling.
- App tests verify manifest-driven launch behavior and third-party native rejection.
- Full local quality gate: `make ci`.

## Follow-up

- Add host API version negotiation/feature flags for ABI evolution.
- Add richer telemetry for capability denials and prompt/decision outcomes.
- Evaluate signature verification integration for stronger marketplace trust posture.
