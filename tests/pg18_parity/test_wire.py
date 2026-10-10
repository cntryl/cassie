"""Independent PostgreSQL packets and reference lifecycle acceptance."""

import copy
import json
import pathlib
import signal
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2] / "tools"))
from pg18_parity.comparison import compare
from pg18_parity.libpq import Libpq
from pg18_parity.reference import reference, verify_artifact

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = pathlib.Path("/opt/homebrew/opt/postgresql@18/bin")
LIB = BIN.parent / "lib/postgresql/libpq.dylib"
MANIFEST = json.loads((ROOT / "tests/corpus/postgresql18/reference.json").read_text())


class WireTests(unittest.TestCase):
    def should_reject_a_changed_reference_artifact_before_startup(self):
        # Arrange
        wrong = copy.deepcopy(MANIFEST)
        wrong["executables"]["postgres"] = "0" * 64
        # Act / Assert
        with self.assertRaisesRegex(ValueError, "artifact mismatch"):
            verify_artifact(BIN, wrong)

    def should_preserve_reference_bytes_descriptors_parameters_and_results(self):
        # Arrange
        libpq = Libpq(LIB)
        evidence = ROOT / "target/pg18-wire"
        evidence.mkdir(parents=True, exist_ok=True)
        path = pathlib.Path(tempfile.mkdtemp(prefix="run-", dir=evidence)) / "ref"
        previous_handler = signal.getsignal(signal.SIGTERM)
        # Act
        with reference(BIN, MANIFEST, path, libpq) as dsn:
            with libpq.connect(dsn) as conn:
                simple = conn.query("SELECT NULL::text AS absent, 'NULL'::text AS literal")
                binary = conn.query("SELECT $1::bigint AS exact", "extended_binary",
                                    [{"oid": 20, "format": 1, "value": "0020000000000001"}])
                multiple = conn.query("SELECT 1; SELECT 2;")
                error = conn.query("SELECT missing_parity_column", "extended_text")
                duplicate = conn.query("SELECT 1 AS n UNION ALL SELECT 1 AS n")
        # Assert
        self.assertEqual(simple[0]["rows"], [[None, "4e554c4c"]])
        self.assertEqual(binary[0]["rows"], [["0020000000000001"]])
        self.assertEqual(binary[0]["parameter_oids"], [20])
        self.assertEqual(binary[0]["columns"], [{"name": "exact", "name_hex": "6578616374", "oid": 20,
                                                "typmod": -1, "size": 8, "format": 1}])
        self.assertEqual(binary[0]["prepared_columns"][0]["oid"], 20)
        self.assertEqual([result["rows"] for result in multiple], [[["31"]], [["32"]]])
        self.assertEqual(error[0]["sqlstate"], "42703")
        self.assertEqual(duplicate[0]["rows"], [["31"], ["31"]])
        self.assertFalse((path / "data/postmaster.pid").exists())
        self.assertEqual(signal.getsignal(signal.SIGTERM), previous_handler)
        self.assertEqual(compare(binary[0], binary[0], ordered=True), [])


if __name__ == "__main__":
    loader = unittest.TestLoader()
    loader.testMethodPrefix = "should_"
    result = unittest.TextTestRunner(verbosity=2).run(loader.loadTestsFromTestCase(WireTests))
    sys.exit(not result.wasSuccessful())
