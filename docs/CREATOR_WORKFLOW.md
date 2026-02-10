# Creator Workflow

This guide documents the current CLI-first creator workflow for milestone `M7`.

## Starter Template

Generate a starter scaffold with creator CLI:

```bash
cargo run -p dark-forest -- --init-template /tmp/my-wasm-game --id my-wasm-game --name "My WASM Game" --author "Your Name" --version 0.1.0
```

Manual fallback using the starter scaffold in `templates/wasm-basic`:

```bash
cp -R templates/wasm-basic /tmp/my-wasm-game
```

Then edit `/tmp/my-wasm-game/game.json`:

- set a unique `id`
- set `name`, `author`, and `version`
- keep `permissions` declared (empty array is valid)
- keep `host_api` compatible with host API `0.1.0` (for example `^0.1`)

The template includes a placeholder `main.wasm` file. Replace it with your compiled WASM module.

## Hot-Reload Dev Loop

Run one cycle:

```bash
cargo run -p dark-forest -- --dev /tmp/my-wasm-game --index file:///tmp/index.json
```

Run watch mode for iterative local changes:

```bash
cargo run -p dark-forest -- --dev /tmp/my-wasm-game --index file:///tmp/index.json --watch --interval-ms 500
```

Watch mode computes a signature over game files and reruns the dev cycle when bytes change.

## Deterministic Packaging + Verification

Package + metadata:

```bash
cargo run -p dark-forest -- --pack /tmp/my-wasm-game --out /tmp/my-wasm-game-0.1.0.tar.gz --metadata-out /tmp/my-wasm-game-0.1.0.metadata.json
```

Verify artifact:

```bash
cargo run -p dark-forest -- --verify-artifact /tmp/my-wasm-game-0.1.0.tar.gz --metadata /tmp/my-wasm-game-0.1.0.metadata.json
```

## Publish Modes

Publish to local index:

```bash
cargo run -p dark-forest -- --publish /tmp/my-wasm-game-0.1.0.tar.gz --index file:///tmp/index.json --metadata /tmp/my-wasm-game-0.1.0.metadata.json
```

Publish plan only:

```bash
cargo run -p dark-forest -- --publish /tmp/my-wasm-game-0.1.0.tar.gz --index file:///tmp/index.json --metadata /tmp/my-wasm-game-0.1.0.metadata.json --dry-run
```

Replace existing version with different checksum:

```bash
cargo run -p dark-forest -- --publish /tmp/my-wasm-game-0.1.0.tar.gz --index file:///tmp/index.json --metadata /tmp/my-wasm-game-0.1.0.metadata.json --replace
```

## Publish Quality Gates

Creator publish rejects artifacts when:

- `game.json` omits `permissions`
- `game.json` `host_api` range is incompatible with host API `0.1.0`

These checks run before index/artifact mutation.
