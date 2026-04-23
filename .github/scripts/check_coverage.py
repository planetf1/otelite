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


def _extract_line_percent(summary: dict) -> float:
    lines = summary.get("lines")
    if not isinstance(lines, dict):
        raise ValueError("coverage summary did not contain a 'lines' section")
    percent = lines.get("percent")
    if percent is None:
        raise ValueError("coverage summary lines section did not contain 'percent'")
    return float(percent)


def _normalise_package_name(raw: str) -> str:
    name = raw.strip()
    if name.startswith("crates/"):
        return name.split("/")[-1]
    return name


def _find_package_percentages(data: dict) -> dict[str, float]:
    packages: dict[str, float] = {}

    for entry in data.get("data", []):
        if not isinstance(entry, dict):
            continue

        summary = entry.get("summary")
        if not isinstance(summary, dict):
            continue

        raw_name = (
            entry.get("package_name")
            or entry.get("package")
            or entry.get("name")
            or entry.get("manifest_name")
            or entry.get("manifest_path")
            or ""
        )
        if not raw_name:
            continue

        name = _normalise_package_name(str(raw_name))
        try:
            packages[name] = _extract_line_percent(summary)
        except ValueError:
            continue

    return packages


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: check_coverage.py <coverage.json>", file=sys.stderr)
        return 2

    report_path = Path(sys.argv[1])
    data = json.loads(report_path.read_text())

    thresholds = _load_thresholds()
    workspace = _extract_line_percent(data["summary"])
    package_percentages = _find_package_percentages(data)

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
            failures.append(f"package coverage for {package_name} was not present in coverage.json")
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
