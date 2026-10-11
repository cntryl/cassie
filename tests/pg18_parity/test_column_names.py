"""Reference acceptance for lossless protocol column-name bytes."""

import json
import pathlib
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))
from pg18_parity.libpq import Libpq
from pg18_parity.reference import reference

BIN = pathlib.Path("/opt/homebrew/opt/postgresql@18/bin")
LIB = BIN.parent / "lib/postgresql/libpq.dylib"
MANIFEST = json.loads((ROOT / "tests/corpus/postgresql18/reference.json").read_text())


class ColumnNameTests(unittest.TestCase):
    def should_preserve_sql_ascii_column_bytes_in_simple_and_extended_descriptors(self):
        # Arrange
        lib = Libpq(LIB)
        parent = ROOT / "target/pg18-names"
        parent.mkdir(parents=True, exist_ok=True)
        output = pathlib.Path(tempfile.mkdtemp(prefix="run-", dir=parent)) / "ref"
        raw = b'CREATE VIEW encoding_name AS SELECT 1 AS "\xff", 2 AS "\\xff"'
        setup = "DO $$BEGIN EXECUTE convert_from(decode('" + raw.hex() + "','hex'),'SQL_ASCII'); END$$"
        observations = []
        # Act
        with reference(BIN, MANIFEST, output, lib) as dsn:
            with lib.connect(dsn) as conn:
                created = conn.query("CREATE DATABASE parity_ascii ENCODING 'SQL_ASCII' "
                                     "LC_COLLATE 'C' LC_CTYPE 'C' TEMPLATE template0")
            with lib.connect(dsn.replace("dbname=postgres", "dbname=parity_ascii")) as conn:
                defined = conn.query(setup)
                for mode in ("simple_text", "extended_text", "extended_binary"):
                    observations.extend(conn.query("SELECT * FROM encoding_name", mode))
        # Assert
        self.assertIsNone(created[0]["sqlstate"])
        self.assertIsNone(defined[0]["sqlstate"])
        self.assertEqual(len(observations), 3)
        for observed in observations:
            self.assertIsNone(observed["sqlstate"])
            self.assertEqual([col["name_hex"] for col in observed["columns"]], ["ff", "5c786666"])
            self.assertEqual([col["name"] for col in observed["columns"]], [None, "\\xff"])
            if "prepared_columns" in observed:
                self.assertEqual([col["name_hex"] for col in observed["prepared_columns"]], ["ff", "5c786666"])
        self.assertFalse((output / "data/postmaster.pid").exists())


if __name__ == "__main__":
    loader = unittest.TestLoader()
    loader.testMethodPrefix = "should_"
    result = unittest.TextTestRunner(verbosity=2).run(loader.loadTestsFromTestCase(ColumnNameTests))
    sys.exit(not result.wasSuccessful())
