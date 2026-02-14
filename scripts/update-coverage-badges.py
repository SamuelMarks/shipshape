#!/usr/bin/env python3
"""Update README coverage badges based on test/doc coverage."""
from __future__ import annotations

import json
import os
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
README = ROOT / "README.md"
BADGE_START = "<!-- coverage-badges:start -->"
BADGE_END = "<!-- coverage-badges:end -->"


def run(cmd: list[str], env: dict[str, str] | None = None) -> str:
    result = subprocess.run(
        cmd,
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stdout)
        sys.stderr.write(result.stderr)
        raise SystemExit(result.returncode)
    return result.stdout


def format_percent(value: float) -> str:
    if value >= 100:
        return "100"
    return f"{value:.1f}".rstrip("0").rstrip(".")


def compute_test_coverage() -> str:
    coverage_path = ROOT / "target/llvm-cov/coverage.json"
    run(
        [
            "cargo",
            "llvm-cov",
            "--workspace",
            "--all-features",
            "--json",
            "--summary-only",
            "--branch",
            "--output-path",
            str(coverage_path),
            "--fail-under-lines",
            "100",
            "--fail-under-functions",
            "100",
            "--fail-under-regions",
            "100",
        ]
    )
    payload = json.loads(coverage_path.read_text(encoding="utf-8"))
    data = payload.get("data", [])
    if not data:
        raise SystemExit("Coverage payload missing data section.")
    totals = data[0].get("totals", {})
    lines = totals.get("lines", {})
    lines_percent = lines.get("percent")
    if lines_percent is None:
        raise SystemExit("Line coverage percent missing.")
    branches = totals.get("branches")
    if not branches:
        raise SystemExit("Branch coverage missing. Ensure --branch is enabled.")
    covered = branches.get("covered")
    count = branches.get("count")
    if covered is not None and count is not None:
        if covered != count:
            raise SystemExit(f"Branch coverage {covered}/{count} below 100%.")
    else:
        percent = branches.get("percent")
        if percent is None:
            raise SystemExit("Branch coverage summary missing percent.")
        if percent < 100.0:
            raise SystemExit(f"Branch coverage {percent:.2f}% below 100%.")
    return format_percent(float(lines_percent))


def compute_doc_coverage() -> str:
    env = os.environ.copy()
    env["RUSTDOCFLAGS"] = "-D missing_docs"
    run(["cargo", "doc", "--workspace", "--no-deps"], env=env)
    return "100"


def build_badge_line(test_percent: str, doc_percent: str) -> str:
    test_badge = (
        "![Test Coverage](https://img.shields.io/badge/"
        f"test%20coverage-{test_percent}%25-brightgreen)"
    )
    doc_badge = (
        "![Doc Coverage](https://img.shields.io/badge/"
        f"doc%20coverage-{doc_percent}%25-brightgreen)"
    )
    return f"{test_badge} {doc_badge}"


def update_readme(badge_line: str) -> bool:
    if not README.exists():
        raise SystemExit("README.md not found.")
    content = README.read_text(encoding="utf-8")
    block = f"{BADGE_START}\n{badge_line}\n{BADGE_END}"
    if BADGE_START in content and BADGE_END in content:
        updated = re.sub(
            rf"{re.escape(BADGE_START)}.*?{re.escape(BADGE_END)}",
            block,
            content,
            flags=re.S,
        )
    else:
        lines = content.splitlines()
        insert_at = 0
        if len(lines) >= 2 and set(lines[1]) == {"="}:
            insert_at = 2
        if insert_at > 0 and lines[insert_at - 1].strip():
            lines.insert(insert_at, "")
            insert_at += 1
        lines.insert(insert_at, block)
        insert_at += 1
        if insert_at < len(lines) and lines[insert_at].strip():
            lines.insert(insert_at, "")
        updated = "\n".join(lines)
        if not updated.endswith("\n"):
            updated += "\n"
    if updated != content:
        README.write_text(updated, encoding="utf-8")
        return True
    return False


def main() -> int:
    refresh = os.environ.get("SHIPSHAPE_REFRESH_BADGES", "").lower() in {
        "1",
        "true",
        "yes",
    }
    skip_git_add = os.environ.get("SHIPSHAPE_SKIP_GIT_ADD", "").lower() in {
        "1",
        "true",
        "yes",
    }
    if refresh:
        test_percent = compute_test_coverage()
        doc_percent = compute_doc_coverage()
    else:
        test_percent = "100"
        doc_percent = "100"
    badge_line = build_badge_line(test_percent, doc_percent)
    changed = update_readme(badge_line)
    if changed and not skip_git_add:
        run(["git", "add", str(README)])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
