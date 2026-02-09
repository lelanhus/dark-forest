#!/usr/bin/env python3
"""Fallback typo checker when codespell is unavailable.

This intentionally checks a conservative set of high-confidence typos.
"""

from __future__ import annotations

import argparse
import configparser
import pathlib
import re
import sys

TYPO_MAP = {
    "teh": "the",
    "recieve": "receive",
    "recieved": "received",
    "seperate": "separate",
    "occured": "occurred",
    "occurrance": "occurrence",
    "definately": "definitely",
    "enviroment": "environment",
    "goverment": "government",
    "accomodate": "accommodate",
    "adress": "address",
    "arguement": "argument",
    "beleive": "believe",
    "calender": "calendar",
    "commited": "committed",
    "independant": "independent",
    "persistance": "persistence",
    "relevent": "relevant",
    "sucess": "success",
    "untill": "until",
    "wierd": "weird",
}

WORD_RE = re.compile(r"[A-Za-z][A-Za-z']+")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--config", default=".codespellrc")
    return parser.parse_args()


def load_config(path: pathlib.Path) -> tuple[set[str], list[str]]:
    ignore_words = set()
    skip_paths: list[str] = [".git", "target", "LICENSE-APACHE"]

    if not path.exists():
        return ignore_words, skip_paths

    parser = configparser.ConfigParser()
    parser.read(path)
    section = parser["codespell"] if "codespell" in parser else {}

    raw_ignore = section.get("ignore-words-list", "")
    for word in raw_ignore.split(","):
        cleaned = word.strip().lower()
        if cleaned:
            ignore_words.add(cleaned)

    raw_skip = section.get("skip", "")
    for item in raw_skip.split(","):
        cleaned = item.strip().lstrip("./")
        if cleaned:
            skip_paths.append(cleaned)

    return ignore_words, skip_paths


def should_skip(path: pathlib.Path, skip_parts: list[str]) -> bool:
    normalized = str(path).replace("\\", "/")
    return any(part in normalized for part in skip_parts)


def iter_candidate_files(skip_parts: list[str]) -> list[pathlib.Path]:
    out: list[pathlib.Path] = []
    for path in pathlib.Path(".").rglob("*"):
        if not path.is_file():
            continue
        if should_skip(path, skip_parts):
            continue
        if path.suffix.lower() in {".md", ".txt", ".rs", ".toml", ".yml", ".yaml"}:
            out.append(path)
    return out


def find_typos(text: str, ignore_words: set[str]) -> list[tuple[str, str]]:
    issues: list[tuple[str, str]] = []
    for word in WORD_RE.findall(text):
        lowered = word.lower()
        if lowered in ignore_words:
            continue
        if lowered in TYPO_MAP:
            issues.append((word, TYPO_MAP[lowered]))
    return issues


def main() -> int:
    args = parse_args()
    config_path = pathlib.Path(args.config)
    ignore_words, skip_parts = load_config(config_path)

    failures = 0

    for path in iter_candidate_files(skip_parts):
        try:
            lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()
        except OSError as err:
            print(f"spellcheck fallback: unable to read {path}: {err}", file=sys.stderr)
            failures += 1
            continue

        for idx, line in enumerate(lines, start=1):
            for typo, expected in find_typos(line, ignore_words):
                print(
                    f"{path}:{idx}: possible typo '{typo}' (suggest '{expected}')",
                    file=sys.stderr,
                )
                failures += 1

    if failures:
        print(
            f"spellcheck fallback failed with {failures} potential typo(s).",
            file=sys.stderr,
        )
        return 1

    print("spellcheck fallback passed (high-confidence typo set).")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
