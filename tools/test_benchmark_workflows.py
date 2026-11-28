"""Focused tests for registered owner selection and vector input validation."""

import importlib.util
import os
import unittest
from pathlib import Path
from unittest.mock import patch


MODULE_PATH = Path(__file__).with_name("benchmark_workflows.py")
SPEC = importlib.util.spec_from_file_location("benchmark_workflows", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
workflow = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(workflow)


class BenchmarkWorkflowTests(unittest.TestCase):
    def test_should_select_registered_targets_by_tier_and_category(self) -> None:
        # Arrange
        registry = [
            {"target": "tier1_kernel", "tier": 1, "category": "timed"},
            {"target": "tier1_product", "tier": 1, "category": "product"},
            {"target": "tier2_query", "tier": 2, "category": "timed"},
        ]

        # Act
        targets = workflow.select_targets(registry, {1}, {"timed"})

        # Assert
        self.assertEqual(targets, ["tier1_kernel"])

    def test_should_build_cargo_command_from_registered_package_and_features(self) -> None:
        # Arrange
        record = {
            "package": "cassie-core",
            "target": "tier1_memory",
            "required_features": ["bench-observation"],
        }

        # Act
        command = workflow.cargo_command(record, "timed", Path("target/stress"))

        # Assert
        self.assertEqual(command[:7], ["cargo", "bench", "--locked", "-p", "cassie-core", "--bench", "tier1_memory"])
        self.assertEqual(command[7:9], ["--features", "bench-observation"])
        self.assertEqual(command[-3:], ["--", "--output-dir", str(Path("target/stress").resolve())])

    def test_should_set_absolute_timed_output_and_short_smoke_soak(self) -> None:
        # Arrange
        registry = [{
            "package": "cassie-e2e",
            "target": "tier6_soak_mixed",
            "tier": 6,
            "category": "timed",
            "required_features": [],
        }]

        # Act
        with (
            patch.dict(os.environ, {}, clear=True),
            patch.object(workflow.subprocess, "check_output", return_value="source-head\n"),
            patch.object(workflow.subprocess, "run") as run,
        ):
            workflow.run_target(
                "tier6_soak_mixed", "timed", "smoke", Path("target/stress"), registry
            )

        # Assert
        env = run.call_args.kwargs["env"]
        self.assertEqual(env["STRESS_OUTPUT_DIR"], str(Path("target/stress").resolve()))
        self.assertEqual(env["CASSIE_BENCH_SOAK_DURATION_SECONDS"], "5")

    def test_should_select_the_native_standard_measurement_profile(self) -> None:
        # Arrange
        registry = [{
            "package": "cassie-core",
            "target": "tier1_kernel",
            "tier": 1,
            "category": "timed",
            "required_features": [],
        }]

        # Act
        with (
            patch.object(workflow.subprocess, "check_output", return_value="source-head\n"),
            patch.object(workflow.subprocess, "run") as run,
        ):
            workflow.run_target(
                "tier1_kernel", "timed", "standard", Path("target/stress"), registry
            )

        # Assert
        self.assertEqual(run.call_args.kwargs["env"]["STRESS_PROFILE"], "default")

    def test_should_parse_all_and_selected_tiers(self) -> None:
        # Arrange / Act / Assert
        self.assertEqual(workflow.parse_tiers("all"), set(range(1, 7)))
        self.assertEqual(workflow.parse_tiers("1,3,6"), {1, 3, 6})
        with self.assertRaises(ValueError):
            workflow.parse_tiers("1,7")

    def test_should_accept_registered_vector_configuration_and_range(self) -> None:
        # Arrange / Act / Assert
        for configuration in workflow.VECTOR_CONFIGURATIONS:
            workflow.validate_vector_selector(configuration, "250..500")

    def test_should_reject_invalid_vector_configuration_or_range(self) -> None:
        # Arrange / Act / Assert
        with self.assertRaisesRegex(ValueError, "unknown vector configuration"):
            workflow.validate_vector_selector("hnsw-3", "0..250")
        for query_range in ("0..0", "999..1001", "500..250", "1-2", "x..3"):
            with self.subTest(query_range=query_range), self.assertRaises(ValueError):
                workflow.validate_vector_selector("exact", query_range)


if __name__ == "__main__":
    unittest.main()
