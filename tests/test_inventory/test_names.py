"""Correctness and termination of inventory-name normalization."""

import json
import pathlib
import subprocess
import sys
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
from test_inventory import clean_name, is_test_inventory_file, render_planned_tests


class InventoryNameTests(unittest.TestCase):
    def should_exclude_python_bytecode_from_the_test_file_inventory(self):
        # Arrange
        bytecode = ROOT / "tests/test_inventory/__pycache__/test_names.cpython-312.pyc"
        source = ROOT / "tests/test_inventory/test_names.py"

        # Act
        bytecode_is_inventory = is_test_inventory_file(bytecode)
        source_is_inventory = is_test_inventory_file(source)

        # Assert
        self.assertFalse(bytecode_is_inventory)
        self.assertTrue(source_is_inventory)

    def should_link_owner_issue_for_backfill_rows_without_a_current_source(self):
        # Arrange
        planned = [
            {
                "issue": 799,
                "suite": "storage_indexes",
                "category": "text_search",
                "name": "should_rebuild_indexed_lexemes",
                "actual_test": "should_rebuild_indexed_lexemes",
                "file": "tests/storage_indexes/text_search/missing.rs",
                "status": "authored",
                "execution_status": "failed_observed",
            }
        ]

        # Act
        rendered = render_planned_tests(planned)

        # Assert
        self.assertIn("no current source at", rendered)
        self.assertIn(
            "[#799](https://github.com/cntryl/cassie/issues/799)",
            rendered,
        )
        self.assertNotIn(
            "](../../tests/storage_indexes/text_search/missing.rs)",
            rendered,
        )

    def should_select_the_earliest_valid_wave_suffix(self):
        # Arrange
        cases = [
            ("rows_wave_bad!_wave1_wave2", "rows_wave_bad!"),
            ("rows_wave__wave1", "rows_wave_"),
            ("rows_wave9_wave_a_wave2", "rows"),
            ("rows_wave_a١x_wave2", "rows_wave_a١x"),
            ("rows_wave123abc_wave2", "rows_wave123abc"),
            ("rows_wave_waveabc_wave2", "rows"),
            ("rows_wave_١_wave2", "rows_wave_١"),
            ("rows_wave9_wave2__", "rows_wave9_wave2__"),
            ("rowswave9_wave2", "rowswave9"),
        ]
        # Act
        observed = [(name, clean_name(name)) for name, _ in cases]
        # Assert
        self.assertEqual(observed, cases)

    def should_preserve_wave_suffix_grammar(self):
        # Arrange
        cases = [
            ("arrays_wave", "arrays"),
            ("arrays_wave123", "arrays"),
            ("arrays_wave_a9_foo", "arrays"),
            ("arrays_wave_a_wave9", "arrays"),
            ("arrays_wavejunk", "arrays_wavejunk"),
            ("arrays_wave__a", "arrays_wave__a"),
            ("arrays_wave_a!_wave9", "arrays_wave_a!"),
            ("arrays_wave_a!", "arrays_wave_a!"),
            ("arrays_wave١٢", "arrays"),
            ("arrays_wave_a١٢", "arrays"),
            ("arrays_wave_١", "arrays_wave_١"),
            ("arrays_wave²", "arrays_wave²"),
            ("arrays_wave9\n", "arrays\n"),
            ("arrays_wave9\n\n", "arrays_wave9\n\n"),
            ("_wave9", "semantics"),
            ("integration_sql_pg18_arrays_wave_9_compat", "arrays"),
        ]
        # Act
        observed = [(name, clean_name(name)) for name, _ in cases]
        # Assert
        self.assertEqual(observed, cases)

    def should_terminate_given_malformed_wave_suffixes(self):
        # Arrange
        names = ["arrays_wave_" + "00_" * 128 + "!", "arrays_wave" + "9" * 128 + "!"]
        program = (
            "import json,sys; sys.path.insert(0, 'tools'); "
            "from test_inventory import clean_name; "
            "print(json.dumps([clean_name(s) for s in json.load(sys.stdin)]))"
        )
        # Act
        # Bound a stuck parser, rather than asserting a performance threshold.
        result = subprocess.run(
            [sys.executable, "-c", program],
            cwd=ROOT,
            input=json.dumps(names),
            capture_output=True,
            text=True,
            timeout=2,
            check=True,
        )
        # Assert
        self.assertEqual(json.loads(result.stdout), names)


def load_tests(_loader, _tests, _pattern):
    loader = unittest.TestLoader()
    loader.testMethodPrefix = "should_"
    return loader.loadTestsFromTestCase(InventoryNameTests)


if __name__ == "__main__":
    unittest.main(verbosity=2)
