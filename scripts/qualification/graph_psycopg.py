"""Explicit real-client qualification; public Psycopg composite/array adapters only."""
import os
import unittest
from uuid import UUID

import psycopg
from psycopg.types.composite import CompositeInfo, register_composite


class GraphClientQualification(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.conn = psycopg.connect(
            host="127.0.0.1", port=int(os.environ["CASSIE_GRAPH_PORT"]),
            user="root", password=os.environ["CASSIE_GRAPH_PASSWORD"], dbname="postgres",
            sslmode="disable", gssencmode="disable", autocommit=True,
        )
        cls.infos = {}
        # Register dependencies before their parents, using the actual catalog
        # OIDs and ordered attribute types. No codec or wire frame is supplied.
        for name in ("graph_key_part", "graph_element_ref", "graph_path"):
            row = cls.conn.execute(
                "SELECT t.oid, t.typarray FROM pg_type t JOIN pg_namespace n "
                "ON n.oid = t.typnamespace WHERE n.nspname = 'cassie_graph' "
                "AND t.typname = %s", (name,),
            ).fetchone()
            fields = cls.conn.execute(
                "SELECT a.attname, a.atttypid FROM pg_attribute a JOIN pg_type t "
                "ON a.attrelid_oid = t.typrelid JOIN pg_namespace n "
                "ON n.oid = t.typnamespace WHERE n.nspname = 'cassie_graph' "
                "AND t.typname = %s AND a.attnum > 0 ORDER BY a.attnum", (name,),
            ).fetchall()
            if row is None or not fields:
                raise AssertionError(f"missing catalog metadata for {name}")
            info = CompositeInfo(name, int(row[0]), int(row[1]),
                                 regtype=f"cassie_graph.{name}",
                                 field_names=[field[0] for field in fields],
                                 field_types=[int(field[1]) for field in fields])
            register_composite(info, cls.conn)
            cls.infos[name] = info
        cls.text = 'nested,"quote"\\slash雪'
        setup = [
            "CREATE TABLE room (tenant TEXT NOT NULL, seq BIGINT NOT NULL, PRIMARY KEY (tenant, seq))",
            "CREATE TABLE place (id UUID PRIMARY KEY)",
            "CREATE TABLE door (id UUID PRIMARY KEY, tenant TEXT NOT NULL, seq BIGINT NOT NULL, target UUID NOT NULL)",
            "CREATE PROPERTY GRAPH rooms VERTEX TABLES (room AS r KEY (tenant, seq) LABEL room PROPERTIES (tenant, seq), "
            "place AS p KEY (id) LABEL place PROPERTIES (id)) EDGE TABLES (door AS d KEY (id) "
            "SOURCE KEY (tenant, seq) REFERENCES r (tenant, seq) DESTINATION KEY (target) REFERENCES p (id) LABEL door PROPERTIES (id))",
        ]
        for sql in setup:
            cls.conn.execute(sql)
        cls.vertex = cls.conn.execute(
            "SELECT cassie_graph.vertex_ref('rooms', 'r', "
            "ARRAY[cassie_graph.key_part(%s::text), cassie_graph.key_part(%s::bigint)])",
            (cls.text, -(2**63)),
        ).fetchone()[0]
        cls.target = cls.conn.execute(
            "SELECT cassie_graph.vertex_ref('rooms', 'p', ARRAY[cassie_graph.key_part(%s::uuid)])",
            (UUID("11111111-2222-3333-4444-555555555555"),),
        ).fetchone()[0]
        cls.edge = cls.conn.execute(
            "SELECT cassie_graph.edge_ref('rooms', 'd', ARRAY[cassie_graph.key_part(%s::uuid)])",
            (UUID("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),),
        ).fetchone()[0]

    @classmethod
    def tearDownClass(cls):
        cls.conn.close()

    def should_roundtrip_paths_and_arrays_in_prepared_text_and_binary(self):
        # Arrange: zero-hop, heterogeneous one-hop and an extreme bigint key.
        path_type = self.infos["graph_path"].python_type
        key_type = self.infos["graph_key_part"].python_type
        ref_type = self.infos["graph_element_ref"].python_type
        high = ref_type(self.vertex.database_id, self.vertex.graph_id,
                        self.vertex.mapping_id, self.vertex.element_kind,
                        [key_type(4, None, self.text, None), key_type(3, 2**63 - 1, None, None)])
        paths = [path_type([self.vertex], [], []),
                 path_type([high, self.target], [self.edge], [1])]
        for parameter_format in ("t", "b"):
            for binary_result in (False, True):
                with self.subTest(parameter_format=parameter_format, binary_result=binary_result):
                    with self.conn.cursor(binary=binary_result) as cursor:
                        # Act: Psycopg's own dumpers/loaders, prepared Describe/Bind/Execute.
                        cursor.execute(f"SELECT %{parameter_format}::cassie_graph.graph_path, "
                                       f"%{parameter_format}::cassie_graph.graph_path[]",
                                       (paths[1], paths), prepare=True)
                        row = cursor.fetchone()
                        # Assert: exact identities, nested keys, array order and Describe OIDs.
                        self.assertEqual(row, (paths[1], paths))
                        self.assertEqual([column.type_code for column in cursor.description],
                                         [self.infos["graph_path"].oid, self.infos["graph_path"].array_oid])
                        self.assertIsNone(cursor.fetchone())

    def should_read_typed_keys_and_ordered_steps_from_a_bound_path(self):
        # Arrange
        path = self.infos["graph_path"].python_type([self.vertex, self.target], [self.edge], [1])
        # Act
        keys = self.conn.execute(
            "SELECT k.ordinal, k.type_tag, k.integer_value, k.text_value FROM "
            "cassie_graph.key_parts(%s::cassie_graph.graph_element_ref) AS k ORDER BY k.ordinal",
            (self.vertex,), prepare=True).fetchall()
        steps = self.conn.execute(
            "SELECT s.position, s.direction FROM cassie_graph.path_steps(%s::cassie_graph.graph_path) "
            "AS s ORDER BY s.position", (path,), prepare=True).fetchall()
        # Assert
        self.assertEqual(keys, [(1, 4, None, self.text), (2, 3, -(2**63), None)])
        self.assertEqual(steps, [(0, None), (1, 1)])

    def should_reject_a_malformed_bound_path_without_partial_results(self):
        # Arrange
        malformed = self.infos["graph_path"].python_type([self.vertex], [], [1])
        for parameter_format in ("t", "b"):
            # Act / Assert
            with self.subTest(parameter_format=parameter_format):
                with self.assertRaises(psycopg.Error) as raised:
                    self.conn.execute(f"SELECT %{parameter_format}::cassie_graph.graph_path",
                                      (malformed,), prepare=True).fetchall()
                self.assertEqual(raised.exception.sqlstate, "22023")
                self.assertEqual(self.conn.execute("SELECT 1").fetchone(), (1,))


if __name__ == "__main__":
    # should_ names keep the same invariant naming convention as the Rust suites.
    suite = unittest.TestLoader()
    suite.testMethodPrefix = "should_"
    result = unittest.TextTestRunner(verbosity=2).run(suite.loadTestsFromTestCase(GraphClientQualification))
    raise SystemExit(0 if result.wasSuccessful() else 1)
