#!/usr/bin/env python3
"""Deterministic ptybox driver assertions with per-step diagnostics."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from uuid import uuid4

HEARTBEAT_ACTION = {
    "type": "wait",
    "payload": {
        "condition": {"type": "screen_contains", "payload": {"text": ""}},
        "timeout_ms": 50,
    },
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run ptybox driver scenarios and assert terminal markers.",
    )
    parser.add_argument(
        "--ptybox-bin",
        default="ptybox",
        help="Path to ptybox binary.",
    )
    parser.add_argument(
        "--app-bin",
        default=str(Path.cwd() / "target" / "debug" / "dark-forest"),
        help="Path to dark-forest binary used by scenario placeholders.",
    )
    parser.add_argument(
        "--run-root",
        default="/tmp/df-ptybox-runs",
        help="Root directory for persisted run artifacts.",
    )
    parser.add_argument(
        "--scenario",
        action="append",
        required=True,
        help="Scenario JSON path. Repeat to run multiple scenarios.",
    )
    parser.add_argument(
        "--default-timeout-ms",
        type=int,
        default=3000,
        help="Default per-step expectation timeout.",
    )
    parser.add_argument(
        "--default-poll-ms",
        type=int,
        default=75,
        help="Default polling interval while waiting on expectations.",
    )
    parser.add_argument(
        "--timeout-scale",
        type=float,
        default=1.0,
        help="Multiply all step timeout values by this scale.",
    )
    parser.add_argument(
        "--keep-going",
        action="store_true",
        help="Continue running remaining scenarios after a failure.",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Print extra progress details.",
    )
    return parser.parse_args()


def now_stamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%d-%H%M%S")


def scenario_slug(name: str) -> str:
    lowered = name.lower()
    chars = []
    for ch in lowered:
        if ch.isalnum() or ch in {"-", "_"}:
            chars.append(ch)
        elif ch.isspace() or ch in {"/", "."}:
            chars.append("-")
    slug = "".join(chars).strip("-")
    return slug or "scenario"


def substitute(value: Any, ctx: dict[str, str]) -> Any:
    if isinstance(value, str):
        result = value
        for key, replacement in ctx.items():
            result = result.replace("{" + key + "}", replacement)
        return result
    if isinstance(value, list):
        return [substitute(item, ctx) for item in value]
    if isinstance(value, dict):
        return {key: substitute(item, ctx) for key, item in value.items()}
    return value


def normalize_expectation(expect: Any) -> dict[str, list[str]]:
    if expect is None:
        return {"contains": [], "not_contains": []}
    if isinstance(expect, list):
        return {"contains": [str(item) for item in expect], "not_contains": []}
    if isinstance(expect, dict):
        contains = expect.get("contains", [])
        not_contains = expect.get("not_contains", [])
        return {
            "contains": [str(item) for item in contains],
            "not_contains": [str(item) for item in not_contains],
        }
    raise ValueError(f"unsupported expectation value: {expect!r}")


def extract_screen_lines(observation: dict[str, Any]) -> list[str]:
    return observation.get("screen", {}).get("lines", []) or []


def evaluate_expectation(expect: dict[str, list[str]], observation: dict[str, Any]) -> tuple[list[str], list[str]]:
    lines = extract_screen_lines(observation)
    screen_text = "\n".join(lines)
    missing = [needle for needle in expect["contains"] if needle not in screen_text]
    forbidden = [needle for needle in expect["not_contains"] if needle in screen_text]
    return missing, forbidden


def format_screen(lines: list[str]) -> str:
    if not lines:
        return "<no screen lines>"
    return "\n".join(f"{idx + 1:02d}: {line}" for idx, line in enumerate(lines))


class DriverSession:
    def __init__(
        self,
        ptybox_bin: str,
        command: list[str],
        observations_path: Path,
        verbose: bool,
    ) -> None:
        if not command:
            raise ValueError("scenario command must not be empty")

        self.proc = subprocess.Popen(
            [ptybox_bin, "driver", "--stdio", "--json", "--", *command],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.observations_path = observations_path
        self.observation_writer = observations_path.open("w", encoding="utf-8")
        self.verbose = verbose

    def send_action(
        self,
        action: dict[str, Any],
        *,
        step_name: str,
        action_index: int,
    ) -> dict[str, Any]:
        if self.proc.stdin is None or self.proc.stdout is None:
            raise RuntimeError("driver pipes are unavailable")

        envelope = {"protocol_version": 1, "action": action}
        payload = json.dumps(envelope, separators=(",", ":"))
        self.proc.stdin.write(payload + "\n")
        self.proc.stdin.flush()

        line = self.proc.stdout.readline()
        if not line:
            stderr_output = ""
            if self.proc.stderr is not None:
                stderr_output = self.proc.stderr.read().strip()
            raise RuntimeError(
                "ptybox driver closed stdout early"
                + (f"; stderr: {stderr_output}" if stderr_output else "")
            )

        try:
            observation = json.loads(line)
        except json.JSONDecodeError as err:
            raise RuntimeError(f"invalid observation JSON: {err}") from err

        event = {
            "step": step_name,
            "action_index": action_index,
            "action": action,
            "observation": observation,
        }
        self.observation_writer.write(json.dumps(event) + "\n")
        self.observation_writer.flush()

        if self.verbose:
            lines = extract_screen_lines(observation)
            print(
                f"  observation step={step_name!r} action={action_index} rows={len(lines)}",
                file=sys.stderr,
            )

        return observation

    def close(self) -> tuple[int | None, str]:
        if self.proc.stdin is not None and not self.proc.stdin.closed:
            try:
                self.proc.stdin.close()
            except OSError:
                pass

        stderr_output = ""
        if self.proc.stderr is not None:
            stderr_output = self.proc.stderr.read().strip()

        exit_code: int | None
        try:
            exit_code = self.proc.wait(timeout=8)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            exit_code = self.proc.wait()

        self.observation_writer.close()
        return exit_code, stderr_output


class ScenarioFailure(RuntimeError):
    pass


def run_scenario(
    scenario_path: Path,
    args: argparse.Namespace,
    campaign_dir: Path,
) -> dict[str, Any]:
    raw = json.loads(scenario_path.read_text(encoding="utf-8"))
    scenario_name = str(raw.get("name") or scenario_path.stem)
    run_dir = campaign_dir / f"{scenario_slug(scenario_name)}-{uuid4().hex[:8]}"
    run_dir.mkdir(parents=True, exist_ok=True)
    home_dir = run_dir / "home"
    home_dir.mkdir(parents=True, exist_ok=True)

    context = {
        "app_bin": str(Path(args.app_bin).resolve()),
        "workspace": str(Path.cwd().resolve()),
        "run_dir": str(run_dir),
        "home": str(home_dir),
    }
    scenario = substitute(raw, context)
    (run_dir / "scenario.resolved.json").write_text(
        json.dumps(scenario, indent=2) + "\n",
        encoding="utf-8",
    )

    command = scenario.get("command")
    if not isinstance(command, list) or not command:
        raise ScenarioFailure("scenario command must be a non-empty array")

    defaults = scenario.get("defaults", {})
    base_timeout_ms = int(defaults.get("timeout_ms", args.default_timeout_ms))
    base_poll_ms = int(defaults.get("poll_interval_ms", args.default_poll_ms))
    terminate = bool(scenario.get("terminate", True))
    allow_nonzero_exit = bool(scenario.get("allow_nonzero_exit", False))

    observations_path = run_dir / "observations.ndjson"
    summary: dict[str, Any] = {
        "scenario": scenario_name,
        "scenario_path": str(scenario_path),
        "run_dir": str(run_dir),
        "status": "running",
        "steps": [],
        "started_at": datetime.now(timezone.utc).isoformat(),
    }

    session = DriverSession(
        args.ptybox_bin,
        [str(part) for part in command],
        observations_path,
        args.verbose,
    )

    try:
        last_observation: dict[str, Any] | None = None
        steps = scenario.get("steps")
        if not isinstance(steps, list) or not steps:
            raise ScenarioFailure("scenario must include a non-empty steps array")

        for step_index, step in enumerate(steps):
            if not isinstance(step, dict):
                raise ScenarioFailure(f"step {step_index} must be an object")

            step_name = str(step.get("name") or f"step-{step_index + 1}")
            step_timeout_ms = int(
                float(step.get("timeout_ms", base_timeout_ms)) * args.timeout_scale
            )
            step_poll_ms = int(step.get("poll_interval_ms", base_poll_ms))

            actions: list[dict[str, Any]] = []
            if "action" in step:
                action = step.get("action")
                if isinstance(action, dict):
                    actions.append(action)
                else:
                    raise ScenarioFailure(f"step {step_name} has invalid 'action' object")
            if "actions" in step:
                action_list = step.get("actions")
                if not isinstance(action_list, list):
                    raise ScenarioFailure(f"step {step_name} has invalid 'actions' list")
                for action in action_list:
                    if not isinstance(action, dict):
                        raise ScenarioFailure(
                            f"step {step_name} contains non-object action entry"
                        )
                    actions.append(action)

            if args.verbose:
                print(f"running step: {step_name}", file=sys.stderr)

            for action_index, action in enumerate(actions, start=1):
                last_observation = session.send_action(
                    action,
                    step_name=step_name,
                    action_index=action_index,
                )

            expectation = normalize_expectation(step.get("expect"))
            if expectation["contains"] or expectation["not_contains"]:
                if last_observation is None:
                    last_observation = session.send_action(
                        HEARTBEAT_ACTION,
                        step_name=step_name,
                        action_index=0,
                    )

                deadline = time.monotonic() + (step_timeout_ms / 1000.0)
                while True:
                    missing, forbidden = evaluate_expectation(expectation, last_observation)
                    if not missing and not forbidden:
                        break

                    if time.monotonic() >= deadline:
                        lines = extract_screen_lines(last_observation)
                        raise ScenarioFailure(
                            "\n".join(
                                [
                                    f"step '{step_name}' expectation failed",
                                    f"missing contains markers: {missing or '[]'}",
                                    f"found forbidden markers: {forbidden or '[]'}",
                                    "last screen snapshot:",
                                    format_screen(lines),
                                ]
                            )
                        )

                    time.sleep(step_poll_ms / 1000.0)
                    last_observation = session.send_action(
                        HEARTBEAT_ACTION,
                        step_name=step_name,
                        action_index=0,
                    )

            summary["steps"].append(
                {
                    "name": step_name,
                    "timeout_ms": step_timeout_ms,
                    "poll_ms": step_poll_ms,
                    "status": "passed",
                }
            )

        if terminate:
            try:
                session.send_action(
                    {"type": "terminate", "payload": {}},
                    step_name="_finalize",
                    action_index=1,
                )
            except RuntimeError:
                # Child may already be gone due in-app quit actions.
                pass

        exit_code, stderr_output = session.close()
        if not allow_nonzero_exit and exit_code not in (0, None):
            raise ScenarioFailure(
                f"driver exited with code {exit_code}; stderr: {stderr_output or '<empty>'}"
            )

        summary.update(
            {
                "status": "passed",
                "exit_code": exit_code,
                "stderr": stderr_output,
                "finished_at": datetime.now(timezone.utc).isoformat(),
            }
        )
        return summary

    except Exception as err:
        exit_code, stderr_output = session.close()
        summary.update(
            {
                "status": "failed",
                "error": str(err),
                "exit_code": exit_code,
                "stderr": stderr_output,
                "finished_at": datetime.now(timezone.utc).isoformat(),
            }
        )
        return summary


def main() -> int:
    args = parse_args()

    run_root = Path(args.run_root).resolve()
    run_root.mkdir(parents=True, exist_ok=True)
    campaign_id = f"driver-{now_stamp()}-{uuid4().hex[:8]}"
    campaign_dir = run_root / campaign_id
    campaign_dir.mkdir(parents=True, exist_ok=True)

    summaries: list[dict[str, Any]] = []
    failures = 0

    for scenario_arg in args.scenario:
        scenario_path = Path(scenario_arg).resolve()
        if not scenario_path.exists():
            print(f"FAIL {scenario_arg}: file not found", file=sys.stderr)
            failures += 1
            if not args.keep_going:
                break
            continue

        summary = run_scenario(scenario_path, args, campaign_dir)
        summaries.append(summary)
        if summary["status"] == "passed":
            print(f"PASS {summary['scenario']} ({summary['run_dir']})")
        else:
            failures += 1
            print(
                f"FAIL {summary['scenario']} ({summary['run_dir']}): {summary.get('error', 'unknown error')}",
                file=sys.stderr,
            )
            if not args.keep_going:
                break

    report = {
        "campaign_dir": str(campaign_dir),
        "scenarios": summaries,
        "failures": failures,
        "status": "passed" if failures == 0 else "failed",
        "generated_at": datetime.now(timezone.utc).isoformat(),
    }
    report_path = campaign_dir / "report.json"
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

    print(f"report={report_path}")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
