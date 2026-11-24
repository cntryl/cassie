"""Acceptance for isolated SQL fixtures in differential corpus cells."""

import contextlib
import pathlib
import sys
import unittest
import uuid
from unittest.mock import patch

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2] / "tools"))
from pg18_parity.run import run_cells


class Connection:
    def __init__(self, results):
        self.results = iter(results)
        self.calls = []

    def query(self, sql, mode="simple_text", parameters=()):
        self.calls.append((sql, mode, list(parameters)))
        return next(self.results)


class Connections:
    def __init__(self, *connections):
        self.connections = iter(connections)

    def connect(self, _dsn):
        return contextlib.nullcontext(next(self.connections))


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.success = [{"status": 1, "sqlstate": None, "command_tag": "BEGIN",
                         "columns": [], "rows": [], "parameter_oids": []}]
        self.value = [{"status": 2, "sqlstate": None, "command_tag": "INSERT 0 1",
                       "columns": [], "rows": [["31"]], "parameter_oids": []}]
        self.cell = {"id": "returning:fixture", "owner": 869, "ordered": False,
                     "setup": ["BEGIN", "CREATE TABLE fixture(id BIGINT)"],
                     "sql": "INSERT INTO fixture VALUES($1) RETURNING id",
                     "parameters": [{"oid": 20, "format": 0, "value": "31"}],
                     "modes": ["extended_binary"]}

    def should_prepare_each_connection_before_its_selected_wire_query(self):
        # Arrange
        oracle = Connection([self.success, self.success, self.value])
        cassie = Connection([self.success, self.success, self.value])
        # Act
        outcomes = run_cells(Connections(oracle, cassie), "oracle", "cassie", [self.cell])
        # Assert
        expected = [(sql, "simple_text", []) for sql in self.cell["setup"]]
        expected.append((self.cell["sql"], "extended_binary", self.cell["parameters"]))
        self.assertEqual(oracle.calls, expected)
        self.assertEqual(cassie.calls, expected)
        self.assertEqual(outcomes[0]["outcome"], "passed")
        self.assertEqual(outcomes[0]["reference"], self.value)

    def should_fail_a_cell_when_its_cassie_fixture_errors(self):
        # Arrange
        error = [{**self.success[0], "status": 7, "sqlstate": "42601"}]
        oracle = Connection([self.success, self.success, self.value])
        cassie = Connection([self.success, error])
        # Act
        outcomes = run_cells(Connections(oracle, cassie), "oracle", "cassie", [self.cell])
        # Assert
        self.assertEqual(outcomes[0]["outcome"], "failed")
        self.assertEqual(outcomes[0]["stage"], "setup")
        self.assertEqual(outcomes[0]["side"], "cassie")
        self.assertEqual(outcomes[0]["setup_index"], 1)
        self.assertEqual(outcomes[0]["results"], error)
        self.assertFalse(any(sql == self.cell["sql"] for sql, _, _ in cassie.calls))

    def should_fail_a_cell_when_its_reference_fixture_returns_no_result(self):
        # Arrange
        oracle = Connection([[]])
        cassie = Connection([self.success, self.success, self.value])
        # Act
        outcomes = run_cells(Connections(oracle, cassie), "oracle", "cassie", [self.cell])
        # Assert
        self.assertEqual(outcomes[0]["outcome"], "failed")
        self.assertEqual(outcomes[0]["side"], "reference")
        self.assertEqual(outcomes[0]["setup_index"], 0)
        self.assertEqual(cassie.calls, [])

    def should_keep_existing_cells_without_fixtures_compatible(self):
        # Arrange
        cell = {**self.cell, "setup": []}
        oracle = Connection([self.value])
        cassie = Connection([self.value])
        # Act
        outcomes = run_cells(Connections(oracle, cassie), "oracle", "cassie", [cell])
        # Assert
        self.assertEqual(outcomes[0]["outcome"], "passed")
        self.assertEqual(len(oracle.calls), 1)
        self.assertEqual(len(cassie.calls), 1)

    def should_use_one_safe_namespace_per_pair_and_fresh_names_per_mode(self):
        # Arrange
        cell = {**self.cell, "setup": ["CREATE TABLE {fixture}_rt(id BIGINT)"],
                "sql": "INSERT INTO {fixture}_rt VALUES($1) RETURNING id",
                "modes": ["extended_text", "extended_binary"]}
        connections = [Connection([self.success, self.value]) for _ in range(4)]
        # Act
        with patch("uuid.uuid4", side_effect=[uuid.UUID(int=1), uuid.UUID(int=2)]):
            outcomes = run_cells(Connections(*connections), "oracle", "cassie", [cell])
        # Assert
        self.assertEqual([row["outcome"] for row in outcomes], ["passed", "passed"])
        for index, namespace in enumerate(["p18_" + "0" * 31 + "1", "p18_" + "0" * 31 + "2"]):
            expected = [(f"CREATE TABLE {namespace}_rt(id BIGINT)", "simple_text", []),
                        (f"INSERT INTO {namespace}_rt VALUES($1) RETURNING id",
                         cell["modes"][index], cell["parameters"])]
            self.assertEqual(connections[index * 2].calls, expected)
            self.assertEqual(connections[index * 2 + 1].calls, expected)
            self.assertEqual(outcomes[index]["fixture_namespace"], namespace)
        self.assertIn("{fixture}", cell["sql"], "input templates stay immutable")


if __name__ == "__main__":
    loader = unittest.TestLoader()
    loader.testMethodPrefix = "should_"
    result = unittest.TextTestRunner(verbosity=2).run(loader.loadTestsFromTestCase(FixtureTests))
    sys.exit(not result.wasSuccessful())
