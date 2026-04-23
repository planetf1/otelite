#!/usr/bin/env python3
"""Validate workspace and crate coverage thresholds from cargo-llvm-cov JSON output."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path


def _load_thresholds() -> dict[str, float]:
    thresholds = {
        "workspace": float(os.environ["COVERAGE_THRESHOLD_WORKSPACE"]),
        "otelite": float(os.environ["COVERAGE_THRESHOLD_OTELITE"]),
        "otelite-core": float(os.environ["COVERAGE_THRESHOLD_OTELITE_CORE"]),
        "otelite-api": float(os.environ["COVERAGE_THRESHOLD_OTELITE_API"]),
        "otelite-client": float(os.environ["COVERAGE_THRESHOLD_OTELITE_CLIENT"]),
        "otelite-receiver": float(os.environ["COVERAGE_THRESHOLD_OTELITE_RECEIVER"]),
        "otelite-storage": float(os.environ["COVERAGE_THRESHOLD_OTELITE_STORAGE"]),
        "otelite-tui": float(os.environ["COVERAGE_THRESHOLD_OTELITE_TUI"]),
    }
    return thresholds


def _line_percent(lines: dict) -> float:
    count = lines.get("count", 0)
    covered = lines.get("covered", 0)
    if count == 0:
        return 100.0
    return covered / count * 100.0


def _workspace_percent(data: dict) -> float:
    # LLVM coverage JSON: data["data"][0]["totals"]["lines"]
    totals = data["data"][0]["totals"]["lines"]
    return _line_percent(totals)


def _crate_percentages(data: dict) -> dict[str, float]:
    """Aggregate per-file line counts into per-crate percentages.

    Files are grouped by the crate directory name extracted from their path
    (the segment after 'crates/' in the filename).
    """
    counts: dict[str, list[int]] = {}  # crate -> [covered, total]

    for file_entry in data["data"][0].get("files", []):
        filename: str = file_entry.get("filename", "")
        # Match paths like .../crates/otelite-core/src/lib.rs
        parts = filename.replace("\\", "/").split("/")
        try:
            idx = parts.index("crates")
            crate_name = parts[idx + 1]
        except (ValueError, IndexError):
            continue

        lines = file_entry.get("summary", {}).get("lines", {})
        covered = lines.get("covered", 0)
        count = lines.get("count", 0)
        if crate_name not in counts:
            counts[crate_name] = [0, 0]
        counts[crate_name][0] += covered
        counts[crate_name][1] += count

    return {
        name: (vals[0] / vals[1] * 100.0 if vals[1] > 0 else 100.0)
        for name, vals in counts.items()
    }


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: check_coverage.py <coverage.json>", file=sys.stderr)
        return 2

    report_path = Path(sys.argv[1])
    data = json.loads(report_path.read_text())

    thresholds = _load_thresholds()
    workspace = _workspace_percent(data)
    package_percentages = _crate_percentages(data)

    print("Coverage threshold report")
    print("=========================")
    print(f"workspace: {workspace:.2f}% (threshold {thresholds['workspace']:.2f}%)")

    failures: list[str] = []
    if workspace < thresholds["workspace"]:
        failures.append(
            f"workspace coverage {workspace:.2f}% is below threshold {thresholds['workspace']:.2f}%"
        )

    for package_name, threshold in thresholds.items():
        if package_name == "workspace":
            continue

        percent = package_percentages.get(package_name)
        if percent is None:
            failures.append(
                f"package coverage for {package_name} was not present in coverage.json"
            )
            continue

        print(f"{package_name}: {percent:.2f}% (threshold {threshold:.2f}%)")
        if percent < threshold:
            failures.append(
                f"{package_name} coverage {percent:.2f}% is below threshold {threshold:.2f}%"
            )

    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with Path(github_output).open("a", encoding="utf-8") as handle:
            handle.write(f"workspace={workspace:.2f}\n")

    if failures:
        print("\nCoverage threshold failures:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1

    print("\nAll coverage thresholds satisfied.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
