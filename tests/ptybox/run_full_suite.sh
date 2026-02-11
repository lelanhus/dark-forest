#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

PTYBOX_BIN="${PTYBOX_BIN:-/tmp/ptybox-target/debug/ptybox}"
APP_BIN="${APP_BIN:-$WORKSPACE_ROOT/target/debug/dark-forest}"
POLICY_FILE="${POLICY_FILE:-$WORKSPACE_ROOT/tests/ptybox/policy.cli.json}"
DRIVER_ASSERT_SCRIPT="${DRIVER_ASSERT_SCRIPT:-$WORKSPACE_ROOT/scripts/ptybox_driver_assert.py}"
RUN_ROOT="${RUN_ROOT:-/tmp/df-ptybox-runs}"
PASSES="${PASSES:-3}"

ACTIONS=(
  "$WORKSPACE_ROOT/tests/ptybox/actions/shell_routes_overlays.json"
  "$WORKSPACE_ROOT/tests/ptybox/actions/runner_leave_confirm.json"
  "$WORKSPACE_ROOT/tests/ptybox/actions/game_snake_plus.json"
  "$WORKSPACE_ROOT/tests/ptybox/actions/game_tetris_like.json"
  "$WORKSPACE_ROOT/tests/ptybox/actions/game_micro_roguelite.json"
  "$WORKSPACE_ROOT/tests/ptybox/actions/game_maze_chase.json"
  "$WORKSPACE_ROOT/tests/ptybox/actions/game_galactic_invaders.json"
)

if [[ ! -x "$PTYBOX_BIN" ]]; then
  echo "ptybox binary is not executable: $PTYBOX_BIN" >&2
  exit 2
fi

if [[ ! -x "$APP_BIN" ]]; then
  echo "dark-forest binary is not executable: $APP_BIN" >&2
  exit 2
fi

if [[ ! -f "$POLICY_FILE" ]]; then
  echo "policy file not found: $POLICY_FILE" >&2
  exit 2
fi

if [[ ! -x "$DRIVER_ASSERT_SCRIPT" ]]; then
  echo "driver assert script not executable: $DRIVER_ASSERT_SCRIPT" >&2
  exit 2
fi

mkdir -p "$RUN_ROOT"
RUN_BATCH_ID="full-suite-$(date -u +%Y%m%d-%H%M%S)-$RANDOM"
BATCH_DIR="$RUN_ROOT/$RUN_BATCH_ID"
mkdir -p "$BATCH_DIR"

run_exec_case() {
  local pass_dir="$1"
  local name="$2"
  local expected_result="$3"
  local expected_snippet="$4"
  shift 4

  local case_dir="$pass_dir/cli"
  local home_dir="$pass_dir/home"
  local out_file="$case_dir/${name}.json"
  mkdir -p "$case_dir" "$home_dir"

  set +e
  "$PTYBOX_BIN" exec \
    --json \
    --policy "$POLICY_FILE" \
    -- /usr/bin/env "HOME=$home_dir" "$APP_BIN" "$@" \
    >"$out_file"
  local rc=$?
  set -e

  if [[ "$expected_result" == "pass" ]]; then
    if [[ $rc -ne 0 ]]; then
      echo "[FAIL] $name expected success but exited $rc" >&2
      echo "output: $out_file" >&2
      return 1
    fi
    if ! jq -e '.status == "passed"' "$out_file" >/dev/null 2>&1; then
      echo "[FAIL] $name expected status=passed" >&2
      echo "output: $out_file" >&2
      return 1
    fi
  else
    if [[ $rc -eq 0 ]]; then
      echo "[FAIL] $name expected failure but exited 0" >&2
      echo "output: $out_file" >&2
      return 1
    fi
  fi

  if [[ -n "$expected_snippet" ]]; then
    if ! rg -Fq -- "$expected_snippet" "$out_file"; then
      echo "[FAIL] $name missing expected snippet: $expected_snippet" >&2
      echo "output: $out_file" >&2
      return 1
    fi
  fi

  echo "[PASS] $name"
}

prepare_bad_index() {
  local index_in="$1"
  local index_out="$2"
  python3 - "$index_in" "$index_out" <<'PY'
import json
import pathlib
import sys

index_in = pathlib.Path(sys.argv[1])
index_out = pathlib.Path(sys.argv[2])
catalog = json.loads(index_in.read_text(encoding="utf-8"))
version = catalog["games"][0]["versions"][0]
version["checksum_sha256"] = "deadbeef"
index_out.write_text(json.dumps(catalog, indent=2) + "\n", encoding="utf-8")
PY
}

seed_permission_grant() {
  local pass_dir="$1"
  local home_dir="$pass_dir/home"
  local permissions_file="$home_dir/.dark-forest/permissions.json"
  mkdir -p "$(dirname "$permissions_file")"
  cat >"$permissions_file" <<'JSON'
{
  "schema_version": 2,
  "grants": {
    "ptybox-sample": [
      {
        "capability": "net",
        "scope": "Prompt",
        "decision": "allow",
        "remembered": true,
        "granted_at": "2026-02-11T00:00:00Z"
      }
    ]
  }
}
JSON
}

run_driver_matrix() {
  local pass_dir="$1"
  local driver_dir="$pass_dir/driver"
  mkdir -p "$driver_dir"

  local cmd=(
    python3 "$DRIVER_ASSERT_SCRIPT"
    --ptybox-bin "$PTYBOX_BIN"
    --app-bin "$APP_BIN"
    --run-root "$driver_dir"
  )

  local action
  for action in "${ACTIONS[@]}"; do
    cmd+=(--scenario "$action")
  done

  "${cmd[@]}" >"$driver_dir/run.log"
  echo "[PASS] driver-matrix"
}

run_single_pass() {
  local pass_num="$1"
  local pass_dir="$BATCH_DIR/pass-$pass_num"
  mkdir -p "$pass_dir"

  local replays_dir="$pass_dir/replays"
  local creator_dir="$pass_dir/creator"
  local game_dir="$creator_dir/game"
  local out_dir="$creator_dir/out"
  local index_path="$creator_dir/index.json"
  local index_bad_path="$creator_dir/index-bad.json"
  local artifact_path="$out_dir/ptybox-sample-0.1.0.tar.gz"
  local metadata_path="$out_dir/ptybox-sample-0.1.0.metadata.json"
  local index_locator="file://$index_path"

  mkdir -p "$replays_dir" "$out_dir"
  cp "$WORKSPACE_ROOT/fixtures/replays/snake-seed-12345.json" "$replays_dir/"
  cp "$WORKSPACE_ROOT/fixtures/replays/tetris-like-seed-4242.json" "$replays_dir/"
  cp "$WORKSPACE_ROOT/fixtures/replays/maze-chase-seed-9001.json" "$replays_dir/"
  cp "$WORKSPACE_ROOT/fixtures/replays/galactic-invaders-seed-777.json" "$replays_dir/"

  echo "running pass $pass_num in $pass_dir"

  run_exec_case "$pass_dir" "help" "pass" "--install-index <locator> <id>" --help

  run_exec_case "$pass_dir" "replay-snake" "pass" "replay.finished=" \
    --replay "$replays_dir/snake-seed-12345.json"
  run_exec_case "$pass_dir" "replay-tetris" "pass" "replay.finished=" \
    --replay "$replays_dir/tetris-like-seed-4242.json"
  run_exec_case "$pass_dir" "replay-maze-chase" "pass" "replay.finished=" \
    --replay "$replays_dir/maze-chase-seed-9001.json"
  run_exec_case "$pass_dir" "replay-galactic-invaders" "pass" "replay.finished=" \
    --replay "$replays_dir/galactic-invaders-seed-777.json"

  run_exec_case "$pass_dir" "init-template" "pass" "command=init-template" \
    --init-template "$game_dir" \
    --id ptybox-sample \
    --name "Ptybox Sample" \
    --author "Dark Forest" \
    --version 0.1.0

  run_exec_case "$pass_dir" "pack" "pass" "command=pack" \
    --pack "$game_dir" \
    --out "$artifact_path" \
    --metadata-out "$metadata_path"

  run_exec_case "$pass_dir" "verify-artifact" "pass" "command=verify-artifact" \
    --verify-artifact "$artifact_path" \
    --metadata "$metadata_path"

  run_exec_case "$pass_dir" "publish-dry-run" "pass" "publish dry-run ptybox-sample@0.1.0" \
    --publish "$artifact_path" \
    --index "$index_locator" \
    --metadata "$metadata_path" \
    --dry-run

  run_exec_case "$pass_dir" "publish" "pass" "published ptybox-sample@0.1.0" \
    --publish "$artifact_path" \
    --index "$index_locator" \
    --metadata "$metadata_path"

  run_exec_case "$pass_dir" "registry-add" "pass" "registry_count=1" \
    --registry-add "$index_locator"
  run_exec_case "$pass_dir" "registry-list" "pass" "message=1 registries configured" \
    --registry-list
  run_exec_case "$pass_dir" "registry-remove" "pass" "registry_count=0" \
    --registry-remove "$index_locator"
  run_exec_case "$pass_dir" "registry-remove-missing" "fail" "registry not configured" \
    --registry-remove "$index_locator"

  run_exec_case "$pass_dir" "install-local" "pass" "installed ptybox-sample@0.1.0" \
    --install-local "$game_dir" \
    --source "local://ptybox-suite"
  run_exec_case "$pass_dir" "verify-installed-local" "pass" "verified ptybox-sample@0.1.0" \
    --verify ptybox-sample
  run_exec_case "$pass_dir" "remove-local" "pass" "removed ptybox-sample" \
    --remove ptybox-sample

  run_exec_case "$pass_dir" "install-index" "pass" "installed ptybox-sample@0.1.0 from index" \
    --install-index "$index_locator" ptybox-sample --version 0.1.0

  run_exec_case "$pass_dir" "verify-installed-index" "pass" "verified ptybox-sample@0.1.0" \
    --verify ptybox-sample

  run_exec_case "$pass_dir" "remove-index-install" "pass" "removed ptybox-sample" \
    --remove ptybox-sample

  prepare_bad_index "$index_path" "$index_bad_path"
  run_exec_case "$pass_dir" "install-index-bad-checksum" "fail" "checksum mismatch" \
    --install-index "file://$index_bad_path" ptybox-sample --version 0.1.0

  run_exec_case "$pass_dir" "permissions-list-empty" "pass" "command=permissions-list" \
    --permissions-list

  seed_permission_grant "$pass_dir"
  run_exec_case "$pass_dir" "permissions-list-game" "pass" "ptybox-sample|capability=net" \
    --permissions-list ptybox-sample
  run_exec_case "$pass_dir" "permissions-revoke-success" "pass" "message=revoked 1 grant(s) for ptybox-sample:net" \
    --permissions-revoke ptybox-sample --capability net
  run_exec_case "$pass_dir" "permissions-revoke-missing" "fail" "no grants found for ptybox-sample:net" \
    --permissions-revoke ptybox-sample --capability net

  run_exec_case "$pass_dir" "invalid-arg" "fail" "unknown argument" --nope

  run_driver_matrix "$pass_dir"

  echo "pass=$pass_num status=passed" >"$pass_dir/status.txt"
  echo "pass $pass_num complete"
}

pass=1
while [[ "$pass" -le "$PASSES" ]]; do
  run_single_pass "$pass"
  pass=$((pass + 1))
done

echo "full_suite_status=passed"
echo "batch_dir=$BATCH_DIR"
