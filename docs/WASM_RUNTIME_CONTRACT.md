# WASM Runtime Contract (v1)

This document defines the host/guest ABI used by Dark Forest for third-party `entry_type=wasm` games in v1.

## Scope

- Applies to installed third-party WASM games launched by the app runtime.
- Native/process third-party entries remain blocked by trust policy.
- This is a minimal ABI intended for stability and incremental extension.

## Required Manifest Fields

`game.json` must include:

- `entry_type: "wasm"`
- `entry`: relative path to the `.wasm` module within the installed artifact directory

The host rejects absolute paths and parent-directory traversal in `entry`.

## Required Exports

The module must export:

1. `df_init(width: i32, height: i32, seed: i64) -> i32`
2. `df_update(kind: i32, arg0: i32, arg1: i32) -> i32`
3. `df_cell(x: i32, y: i32) -> i32`

Return status:

- `0` means success.
- Non-zero means guest error; host treats this as runtime failure.

Cell encoding:

- `df_cell` returns a Unicode scalar value as `i32`.
- Invalid values are rendered as a space (`' '`).

## Optional Exports

- `df_score() -> i64`
- `df_finished() -> i32` (`0` false, non-zero true)

If absent, host defaults are:

- score = `0`
- finished = `false`

## Event Mapping (`df_update`)

`kind` values:

- `0`: Tick (`arg0 = dt_ms`)
- `1`: KeyChar (`arg0 = unicode scalar`)
- `2`: KeyUp
- `3`: KeyDown
- `4`: KeyLeft
- `5`: KeyRight
- `6`: KeyEnter
- `7`: KeyEsc
- `8`: Resize (`arg0 = width`, `arg1 = height`)
- `9`: FocusGained
- `10`: FocusLost
- `11`: Pause
- `12`: Resume
- `99`: OtherInput

Unused args should be ignored by guest.

## Rendering Semantics

- Host clears frame before calling guest render path.
- Host calls `df_cell(x, y)` for each visible cell in the current runtime dimensions.
- Guest should be deterministic for a given state and inputs.

## Compatibility Notes

- ABI is versioned as `v1` by this document; future fields/events must be additive.
- Host API compatibility checks still rely on manifest `host_api` policy.
