#!/usr/bin/env python3
"""Run registered Cassie benchmark owners locally or from GitHub Actions."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

VECTOR_CONFIGURATIONS = ("exact", "hnsw-1", "hnsw-2", "ivfflat-1", "ivfflat-2")


def select_targets(
    registry: list[dict[str, Any]], tiers: set[int], categories: set[str]
) -> list[str]:
    return sorted(
        record["target"]
        for record in registry
        if record["tier"] in tiers and record["category"] in categories
    )


def parse_tiers(value: str) -> set[int]:
    if value == "all":
        return set(range(1, 7))
    try:
        tiers = {int(item) for item in value.split(",")}
    except ValueError as error:
        raise ValueError("tiers must be all or a comma-separated list from 1 through 6") from error
    if not tiers or not tiers <= set(range(1, 7)):
        raise ValueError("tiers must be all or a comma-separated list from 1 through 6")
    return tiers


def validate_vector_selector(configuration: str, query_range: str) -> None:
    if configuration not in VECTOR_CONFIGURATIONS:
        raise ValueError(f"unknown vector configuration: {configuration}")
    if query_range:
        parts = query_range.split("..")
        if len(parts) != 2 or not all(part.isdecimal() for part in parts):
            raise ValueError("vector query range must be START..END")
        start, end = map(int, parts)
        if start < 0 or end <= start or end > 1000:
            raise ValueError("vector query range must satisfy 0 <= START < END <= 1000")


def cargo_command(
    record: dict[str, Any], category: str, output_dir: Path
) -> list[str]:
    output_dir = output_dir.expanduser().resolve()
    command = [
        "cargo", "bench", "--locked", "-p", record["package"], "--bench", record["target"]
    ]
    features = record.get("required_features", [])
    if features:
        command.extend(["--features", ",".join(features)])
    if category == "timed":
        command.extend(["--", "--output-dir", str(output_dir)])
    return command


def read_registry(registry_path: Path | None) -> list[dict[str, Any]]:
    if registry_path:
        return json.loads(registry_path.read_text())
    output = subprocess.check_output(
        ["cargo", "run", "--locked", "-q", "-p", "cassie-bench-support", "--bin", "cassie-bench", "--", "registry"],
        text=True,
    )
    return json.loads(output)


def run_target(
    target: str, category: str, profile: str, output_dir: Path,
    registry: list[dict[str, Any]],
) -> None:
    if category not in {"timed", "product"} or profile not in {"smoke", "standard", "full"}:
        raise ValueError("category must be timed/product and profile must be smoke/standard/full")
    if category == "product" and profile == "standard":
        raise ValueError("product benchmarks support smoke and full profiles")
    output_dir = output_dir.expanduser().resolve()
    env = os.environ.copy()
    env.setdefault("CASSIE_BENCH_SOURCE_COMMIT", subprocess.check_output(
        ["git", "rev-parse", "HEAD"], text=True
    ).strip())
    env.setdefault("CASSIE_BENCH_RUN_ID", f"{env.get('GITHUB_RUN_ID', 'local')}-{env.get('GITHUB_RUN_ATTEMPT', '1')}")
    env.setdefault("CASSIE_BENCH_JOB_ID", env.get("GITHUB_JOB", target))
    env["STRESS_PROFILE"] = {"smoke": "smoke", "standard": "default", "full": "release"}[profile]
    env["STRESS_OUTPUT_DIR"] = str(output_dir)
    if profile == "smoke" and "CASSIE_BENCH_SOAK_DURATION_SECONDS" not in env:
        env["CASSIE_BENCH_SOAK_DURATION_SECONDS"] = "5"
    if category == "product":
        env["CASSIE_BENCH_PRODUCT_PROFILE"] = profile
        env.setdefault("CASSIE_BENCH_PRODUCT_OUTPUT_DIR", str(output_dir / "product" / target))
    matches = [
        record for record in registry
        if record["target"] == target or f"{record['package']}::{record['target']}" == target
    ]
    if len(matches) != 1:
        raise ValueError(f"target selection {target!r} matched {len(matches)} registry entries")
    record = matches[0]
    if record["category"] != category:
        raise ValueError(f"registered category for {target} is {record['category']}, not {category}")
    subprocess.run(cargo_command(record, category, output_dir), check=True, env=env)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    listing = sub.add_parser("list", help="list registered owners for a tier/category selection")
    listing.add_argument("--registry", type=Path, help="registry JSON; default runs the shared CLI")
    listing.add_argument("--tiers", default="all")
    listing.add_argument("--categories", default="timed")
    run = sub.add_parser("run", help="run one registered owner")
    run.add_argument("target")
    run.add_argument("--category", choices=("timed", "product"), default="timed")
    run.add_argument("--profile", choices=("smoke", "standard", "full"), default="full")
    run.add_argument("--output-dir", type=Path, default=Path("target/stress"))
    run.add_argument("--registry", type=Path, help="registry JSON; default runs the shared CLI")
    vector = sub.add_parser("validate-vector", help="check one allowed vector configuration and ID range")
    vector.add_argument("configuration", choices=VECTOR_CONFIGURATIONS)
    vector.add_argument("query_range")
    try:
        args = parser.parse_args()
        if args.command == "list":
            registry = read_registry(args.registry)
            categories = {value.strip() for value in args.categories.split(",")}
            print("\n".join(select_targets(registry, parse_tiers(args.tiers), categories)))
        elif args.command == "run":
            run_target(
                args.target, args.category, args.profile, args.output_dir,
                read_registry(args.registry),
            )
        else:
            validate_vector_selector(args.configuration, args.query_range)
    except (OSError, json.JSONDecodeError, subprocess.CalledProcessError, ValueError) as error:
        print(f"benchmark workflow error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
