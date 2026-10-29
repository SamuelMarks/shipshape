#!/usr/bin/env python3
"""Pre-commit hook pipeline and README coverage badge updater for ShipShape."""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
UI_ROOT = ROOT / "shipshape-ui"
README = ROOT / "README.md"
BADGE_START = "<!-- coverage-badges:start -->"
BADGE_END = "<!-- coverage-badges:end -->"
DEFAULT_DATABASE_URL = "postgres://rest_user:rest_pass@localhost:5432/shipshape_db"
DEFAULT_TOKEN_KEYS = "nNaCyzp1PqhVXHaRH2RWbaYyjAZ3Dev5o6+WwRAiKK0="


def get_augmented_env() -> dict[str, str]:
    """Return environment augmented with standard toolchain paths and default test variables."""
    env = dict(os.environ)
    for git_var in (
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_AUTHOR_DATE",
        "GIT_AUTHOR_EMAIL",
        "GIT_AUTHOR_NAME",
        "GIT_COMMON_DIR",
        "GIT_COMMITTER_DATE",
        "GIT_COMMITTER_EMAIL",
        "GIT_COMMITTER_NAME",
        "GIT_DIR",
        "GIT_GRAFT_FILE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_INTERNAL_SUPER_PREFIX",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_OBJECT_DIRECTORY",
        "GIT_PREFIX",
        "GIT_REPLACE_REF_BASE",
        "GIT_SHALLOW_FILE",
        "GIT_WORK_TREE",
    ):
        env.pop(git_var, None)
    paths = [p for p in env.get("PATH", "").split(os.pathsep) if p]
    candidate_dirs = [
        pathlib.Path.home() / ".cargo" / "bin",
        pathlib.Path("/opt/homebrew/bin"),
        pathlib.Path("/usr/local/bin"),
    ]
    for directory in reversed(candidate_dirs):
        if directory.exists() and str(directory) not in paths:
            paths.insert(0, str(directory))
    env["PATH"] = os.pathsep.join(paths)
    if not env.get("DATABASE_URL") or env.get("DATABASE_URL", "").endswith("/rest_db"):
        env["DATABASE_URL"] = DEFAULT_DATABASE_URL
    env.setdefault("TEST_DATABASE_URL", env["DATABASE_URL"])
    env.setdefault("SHIPSHAPE_TOKEN_KEYS", DEFAULT_TOKEN_KEYS)
    return env


def run(
    cmd: list[str],
    env: dict[str, str] | None = None,
    cwd: pathlib.Path = ROOT,
    capture_output: bool = True,
) -> str:
    """Execute a command and raise SystemExit on failure."""
    effective_env = env if env is not None else get_augmented_env()
    resolved = shutil.which(cmd[0], path=effective_env.get("PATH"))
    if resolved:
        cmd = [resolved, *cmd[1:]]
    result = subprocess.run(
        cmd,
        cwd=cwd,
        env=effective_env,
        text=True,
        capture_output=capture_output,
    )
    if result.returncode != 0:
        if capture_output:
            if result.stdout:
                sys.stderr.write(result.stdout)
            if result.stderr:
                sys.stderr.write(result.stderr)
        raise SystemExit(result.returncode)
    return result.stdout if capture_output else ""


def get_badge_color(value: float) -> str:
    """Return the shields.io color for a coverage percentage."""
    if value >= 95.0:
        return "brightgreen"
    if value >= 90.0:
        return "green"
    if value >= 80.0:
        return "yellowgreen"
    if value >= 70.0:
        return "yellow"
    if value >= 60.0:
        return "orange"
    return "red"


def format_percent(value: float) -> str:
    """Format a percentage value cleanly for badge display."""
    if value >= 100.0:
        return "100"
    return f"{value:.1f}".rstrip("0").rstrip(".")


def compute_test_coverage(
    strict: bool = False, refresh: bool = True
) -> tuple[str, float]:
    """Compute test coverage via cargo llvm-cov across all crates and return (formatted_percent, float_percent)."""
    coverage_dir = ROOT / "target/llvm-cov"
    coverage_dir.mkdir(parents=True, exist_ok=True)

    crates = [
        ("shipshape-core", ["--lib"]),
        ("shipshape-cli", ["--bins"]),
        ("shipshape-server", ["--bins"]),
    ]

    total_lines = 0
    covered_lines = 0

    for crate_name, target_flags in crates:
        coverage_path = coverage_dir / f"{crate_name}.json"
        if refresh or not coverage_path.exists():
            run(["cargo", "llvm-cov", "clean", "--workspace"])
            cmd = [
                "cargo",
                "llvm-cov",
                "-p",
                crate_name,
                *target_flags,
                "--all-features",
                "--json",
                "--summary-only",
                "--output-path",
                str(coverage_path),
            ]
            if strict:
                cmd.extend(
                    [
                        "--fail-under-lines",
                        "100",
                        "--fail-under-functions",
                        "100",
                    ]
                )
            run(cmd)
        payload = json.loads(coverage_path.read_text(encoding="utf-8"))
        data = payload.get("data", [])
        if not data:
            raise SystemExit(f"Coverage payload for {crate_name} missing data section.")
        totals = data[0].get("totals", {})
        lines = totals.get("lines", {})
        count = lines.get("count")
        covered = lines.get("covered")
        if count is None or covered is None:
            raise SystemExit(f"Line coverage counts missing for {crate_name}.")
        total_lines += int(count)
        covered_lines += int(covered)

    pct = 100.0 if total_lines == 0 else (covered_lines / total_lines) * 100.0
    if pct >= 99.95:
        return "100", 100.0
    return format_percent(pct), pct


def compute_doc_coverage() -> tuple[str, float]:
    """Verify documentation coverage via cargo doc with -D missing_docs."""
    env = get_augmented_env()
    env["RUSTDOCFLAGS"] = "-D missing_docs -D warnings"
    run(
        ["cargo", "doc", "--workspace", "--no-deps", "--document-private-items"],
        env=env,
    )
    return "100", 100.0


def build_test_badge(test_percent: str, test_color: str = "brightgreen") -> str:
    """Build the Markdown shield for test coverage."""
    return (
        "![Test Coverage](https://img.shields.io/badge/"
        f"test%20coverage-{test_percent}%25-{test_color})"
    )


def build_doc_badge(doc_percent: str, doc_color: str = "brightgreen") -> str:
    """Build the Markdown shield for doc coverage."""
    return (
        "![Doc Coverage](https://img.shields.io/badge/"
        f"doc%20coverage-{doc_percent}%25-{doc_color})"
    )


def build_badge_line(
    test_percent: str,
    doc_percent: str,
    test_color: str = "brightgreen",
    doc_color: str = "brightgreen",
) -> str:
    """Build the Markdown line containing the test and doc coverage shields."""
    test_badge = build_test_badge(test_percent, test_color)
    doc_badge = build_doc_badge(doc_percent, doc_color)
    return f"{test_badge} {doc_badge}"


def extract_existing_badges(content: str) -> tuple[str | None, str | None]:
    """Extract existing Test Coverage and Doc Coverage shield strings from README.md."""
    test_match = re.search(
        r"!\[Test Coverage\]\(https://img\.shields\.io/badge/test%20coverage-[^)]+\)",
        content,
    )
    doc_match = re.search(
        r"!\[Doc Coverage\]\(https://img\.shields\.io/badge/doc%20coverage-[^)]+\)",
        content,
    )
    return (
        test_match.group(0) if test_match else None,
        doc_match.group(0) if doc_match else None,
    )


def update_readme(badge_line: str) -> bool:
    """Insert or update the coverage badge block in README.md."""
    if not README.exists():
        raise SystemExit("README.md not found.")
    nl = chr(10)
    content = README.read_text(encoding="utf-8")
    block = f"{BADGE_START}{nl}{badge_line}{nl}{BADGE_END}"
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
        if lines and lines[0].startswith("#"):
            insert_at = 1
        elif len(lines) >= 2 and set(lines[1]) == {"="}:
            insert_at = 2
        while insert_at < len(lines) and not lines[insert_at].strip():
            insert_at += 1
        while insert_at < len(lines) and (
            lines[insert_at].strip().startswith("[![")
            or lines[insert_at].strip().startswith("![")
        ):
            insert_at += 1
        if insert_at > 0 and lines[insert_at - 1].strip():
            lines.insert(insert_at, "")
            insert_at += 1
        lines.insert(insert_at, block)
        insert_at += 1
        if insert_at < len(lines) and lines[insert_at].strip():
            lines.insert(insert_at, "")
        updated = nl.join(lines)
        if not updated.endswith(nl):
            updated += nl
    if updated != content:
        README.write_text(updated, encoding="utf-8")
        return True
    return False


def stage_format() -> int:
    """Run cargo fmt auto-fix and then verify formatting check."""
    run(["cargo", "fmt", "--all"], capture_output=False)
    run(["cargo", "fmt", "--all", "--", "--check"], capture_output=False)
    return 0


def stage_lint() -> int:
    """Run cargo clippy and rustdoc lints."""
    run(
        [
            "cargo",
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        capture_output=False,
    )
    env = get_augmented_env()
    env["RUSTDOCFLAGS"] = "-D missing_docs -D warnings"
    run(
        ["cargo", "doc", "--workspace", "--no-deps", "--document-private-items"],
        env=env,
        capture_output=False,
    )
    return 0


def stage_build() -> int:
    """Build Rust workspace and Angular UI."""
    run(["cargo", "build", "--workspace", "--all-targets"], capture_output=False)
    if (UI_ROOT / "package.json").exists() and (UI_ROOT / "node_modules").exists():
        run(["npm", "run", "build"], cwd=UI_ROOT, capture_output=False)
    return 0


def stage_test() -> int:
    """Run Rust workspace tests and Angular UI tests."""
    run(["cargo", "test", "--workspace", "--all-targets"], capture_output=False)
    if (UI_ROOT / "package.json").exists() and (UI_ROOT / "node_modules").exists():
        run(["npm", "test"], cwd=UI_ROOT, capture_output=False)
        coverage_dir = UI_ROOT / "coverage"
        if coverage_dir.exists():
            subprocess.run(
                ["git", "checkout", "--", str(coverage_dir)],
                cwd=ROOT,
                capture_output=True,
                check=False,
            )
    return 0


def stage_shields(doc_only: bool = False, test_only: bool = False) -> int:
    """Compute/verify doc and test coverage and update shields in README.md."""
    refresh = os.environ.get("SHIPSHAPE_REFRESH_BADGES", "").lower() in {
        "1",
        "true",
        "yes",
    }
    strict = os.environ.get("SHIPSHAPE_STRICT_COVERAGE", "").lower() in {
        "1",
        "true",
        "yes",
    }
    skip_git_add = os.environ.get("SHIPSHAPE_SKIP_GIT_ADD", "").lower() in {
        "1",
        "true",
        "yes",
    }

    run_doc = doc_only or not (doc_only or test_only)
    run_test = test_only or not (doc_only or test_only)

    existing_content = README.read_text(encoding="utf-8") if README.exists() else ""
    existing_test_badge, existing_doc_badge = extract_existing_badges(existing_content)

    test_badge = existing_test_badge or build_test_badge("100", "brightgreen")
    doc_badge = existing_doc_badge or build_doc_badge("100", "brightgreen")

    if run_doc:
        doc_percent, doc_val = compute_doc_coverage()
        doc_badge = build_doc_badge(doc_percent, get_badge_color(doc_val))
    if run_test:
        test_percent, test_val = compute_test_coverage(strict=strict, refresh=refresh)
        test_badge = build_test_badge(test_percent, get_badge_color(test_val))

    badge_line = f"{test_badge} {doc_badge}"
    changed = update_readme(badge_line)
    if changed and not skip_git_add:
        run(["git", "add", str(README)])
    return 0


def main(argv: list[str] | None = None) -> int:
    """CLI entry point for pre-commit stages and badge updates."""
    parser = argparse.ArgumentParser(
        description="ShipShape pre-commit checks and coverage badge updater."
    )
    parser.add_argument(
        "stage",
        nargs="?",
        choices=["fmt", "lint", "build", "test", "shields", "all"],
        default="shields",
        help="Stage to run (default: shields).",
    )
    parser.add_argument(
        "--doc",
        action="store_true",
        help="Check/update '% doc coverage' shield in README.md.",
    )
    parser.add_argument(
        "--test",
        action="store_true",
        help="Check/update '% test coverage' shield in README.md.",
    )
    args = parser.parse_args(argv)

    if args.stage == "fmt":
        return stage_format()
    if args.stage == "lint":
        return stage_lint()
    if args.stage == "build":
        return stage_build()
    if args.stage == "test":
        return stage_test()
    if args.stage == "all":
        stage_format()
        stage_lint()
        stage_build()
        stage_test()
        return stage_shields(doc_only=args.doc, test_only=args.test)
    return stage_shields(doc_only=args.doc, test_only=args.test)


if __name__ == "__main__":
    raise SystemExit(main())
