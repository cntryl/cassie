"""Independent acceptance checks for the PostgreSQL differential comparator."""

import copy
import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2] / "tools"))
from pg18_parity.comparison import compare, qualify
from pg18_parity.inventory import catalog_entries, fields


class ComparisonTests(unittest.TestCase):
    def setUp(self):
        self.result = {
            "status": 2,
            "sqlstate": None,
            "command_tag": "SELECT 3",
            "columns": [{"name": "value", "oid": 23, "typmod": -1, "size": 4, "format": 0}],
            "rows": [[None], ["31"], ["31"]],
            "parameter_oids": [],
        }

    def should_compare_unordered_rows_as_bags_without_losing_duplicates(self):
        # Arrange
        actual = copy.deepcopy(self.result)
        actual["rows"] = [["31"], [None], ["31"]]
        # Act
        equal = compare(self.result, actual, ordered=False)
        actual["rows"].pop()
        different = compare(self.result, actual, ordered=False)
        # Assert
        self.assertEqual(equal, [])
        self.assertEqual(different, ["rows"])

    def should_preserve_declared_order_and_sql_null_identity(self):
        # Arrange
        actual = copy.deepcopy(self.result)
        actual["rows"] = [["31"], [None], ["31"]]
        # Act
        reordered = compare(self.result, actual, ordered=True)
        actual = copy.deepcopy(self.result)
        actual["rows"][0] = ["4e554c4c"]  # Literal text NULL, not SQL NULL.
        null_changed = compare(self.result, actual, ordered=False)
        # Assert
        self.assertEqual(reordered, ["rows"])
        self.assertEqual(null_changed, ["rows"])

    def should_detect_descriptor_parameter_error_and_command_tag_drift(self):
        # Arrange
        actual = copy.deepcopy(self.result)
        actual["columns"][0]["oid"] = 20
        actual["columns"][0]["typmod"] = 7
        actual["parameter_oids"] = [20]
        actual["sqlstate"] = "22003"
        actual["command_tag"] = "SELECT 1"
        # Act
        differences = compare(self.result, actual, ordered=False)
        # Assert
        self.assertEqual(differences, ["sqlstate", "command_tag", "columns", "parameter_oids"])

    def should_reject_unexecuted_missing_duplicate_and_unowned_cells(self):
        # Arrange
        required = {"sql:select", "type:int4"}
        passing = [{"id": "sql:select", "outcome": "passed"}, {"id": "type:int4", "outcome": "passed"}]
        # Act
        complete = qualify(required, passing)
        missing = qualify(required, passing[:1])
        failed = qualify(required, [passing[0], {"id": "type:int4", "outcome": "unavailable"}])
        duplicate = qualify(required, passing + [passing[0]])
        extra = qualify(required, passing + [{"id": "unknown", "outcome": "passed"}])
        # Assert
        self.assertEqual(complete, [])
        self.assertTrue(missing)
        self.assertTrue(failed)
        self.assertTrue(duplicate)
        self.assertTrue(extra)

    def should_retain_catalog_signature_fields_after_quoted_braces(self):
        # Arrange
        source = "# comment {\n[{ oid => '1', proargnames => '{a,b}', proargtypes => 'int4 text', prosrc => 'x\\'y' }]"
        # Act
        entries = list(catalog_entries(source))
        parsed = fields(entries[0])
        # Assert
        self.assertEqual(len(entries), 1)
        self.assertEqual(parsed["proargtypes"], "int4 text")
        self.assertEqual(parsed["proargnames"], "{a,b}")
        self.assertEqual(parsed["prosrc"], "x\\'y")


if __name__ == "__main__":
    loader = unittest.TestLoader()
    loader.testMethodPrefix = "should_"
    result = unittest.TextTestRunner(verbosity=2).run(loader.loadTestsFromTestCase(ComparisonTests))
    sys.exit(not result.wasSuccessful())
