//! Issue #1111: derived indexes on column tables are built from authoritative
//! field values, maintained incrementally, never publish rolled-back writes and
//! answer exactly like an authoritative scan.

use super::pair::{with_pair, Pair, COL, ROW};
use cassie::types::Value;

/// `ix_*` tables carry the indexes; `ixs_*` tables are the unindexed
/// authoritative-scan oracle holding identical data.
const TABLES: [(&str, &str); 4] = [("ix", ROW), ("ix", COL), ("ixs", ROW), ("ixs", COL)];

struct Case<'a> {
    columns: &'a str,
    seed: &'a str,
    index_ddl: &'a [&'a str],
    queries: &'a [&'a str],
    mutations: &'a [&'a str],
    rolled_back: &'a [&'a str],
    explain: &'a str,
    index_marks: &'a [&'a str],
    /// When set, the row-table plan must itself name the index.
    plan_names_index: bool,
}

fn on_every_table(pair: &Pair, template: &str) {
    for (name, mode) in TABLES {
        pair.run(&template.replace("{t}", &format!("{name}_{mode}")));
    }
}

fn normalized(pair: &Pair, sql: &str, ordered: bool) -> Vec<Vec<Value>> {
    if ordered {
        pair.run(sql)
    } else {
        pair.sorted(sql)
    }
}

fn assert_all_tables_agree(pair: &Pair, queries: &[&str], stage: &str) {
    for query in queries {
        let ordered = query.to_ascii_uppercase().contains("ORDER BY");
        let mut results = TABLES.iter().map(|(name, mode)| {
            normalized(
                pair,
                &query.replace("{t}", &format!("{name}_{mode}")),
                ordered,
            )
        });
        let oracle = results.next().expect("oracle");
        for (index, result) in results.enumerate() {
            assert_eq!(result, oracle, "{query} @ {stage} (table #{})", index + 1);
        }
    }
}

fn plan_text(pair: &Pair, sql: &str) -> String {
    match pair
        .run(&format!("EXPLAIN {sql}"))
        .first()
        .and_then(|row| row.first())
    {
        Some(Value::String(plan)) => plan.clone(),
        _ => panic!("expected a textual plan for {sql}"),
    }
}

fn run_case(case: &Case) {
    with_pair(|pair| {
        pair.create("ix", case.columns, case.seed);
        pair.create("ixs", case.columns, case.seed);
        // Build on the populated tables.
        for ddl in case.index_ddl {
            for mode in [ROW, COL] {
                pair.run(&ddl.replace("{t}", &format!("ix_{mode}")));
            }
        }
        assert_all_tables_agree(pair, case.queries, "build");
        // Incremental maintenance on insert, update and delete.
        for (step, mutation) in case.mutations.iter().enumerate() {
            on_every_table(pair, mutation);
            assert_all_tables_agree(pair, case.queries, &format!("mutation {step}"));
        }
        // Rolled-back writes never publish index entries.
        for (name, mode) in TABLES {
            let table = format!("{name}_{mode}");
            pair.run("BEGIN");
            for statement in case.rolled_back {
                pair.run(&statement.replace("{t}", &table));
            }
            pair.run("ROLLBACK");
        }
        assert_all_tables_agree(pair, case.queries, "rollback");
        // Diagnostics name the same index path for both modes.
        let row_plan = plan_text(pair, &case.explain.replace("{t}", "ix_row"));
        let col_plan = plan_text(pair, &case.explain.replace("{t}", "ix_col"));
        for mark in case.index_marks {
            let row_names = row_plan.contains(&format!("ix_row{mark}"));
            let col_names = col_plan.contains(&format!("ix_col{mark}"));
            assert!(!case.plan_names_index || row_names, "row plan: {row_plan}");
            assert_eq!(col_names, row_names, "column plan: {col_plan}");
        }
    });
}

#[test]
fn should_maintain_scalar_indexes_on_column_tables() {
    // Arrange
    let case = Case {
        columns: "id INT, k TEXT, n INT, active BOOLEAN",
        seed: "(1,'Alpha',10,true),(2,'beta',20,false),(3,NULL,30,true),(4,'Alpha',NULL,true)",
        index_ddl: &[
            "CREATE INDEX {t}_b ON {t} USING btree (k)",
            "CREATE INDEX {t}_h ON {t} USING btree (n)",
            "CREATE INDEX {t}_e ON {t} (lower(k))",
            "CREATE INDEX {t}_p ON {t} (n) WHERE active",
            "CREATE INDEX {t}_c ON {t} (k, n)",
        ],
        queries: &[
            "SELECT id FROM {t} WHERE k = 'Alpha'",
            "SELECT id FROM {t} WHERE k IS NULL",
            "SELECT id FROM {t} WHERE n = 20",
            "SELECT id FROM {t} WHERE n >= 10 AND n < 30 ORDER BY n, id",
            "SELECT id FROM {t} WHERE lower(k) = 'alpha'",
            "SELECT id FROM {t} WHERE active AND n = 30",
            "SELECT id FROM {t} WHERE k = 'Alpha' AND n = 10",
            "SELECT id FROM {t} ORDER BY k, id LIMIT 3",
        ],
        mutations: &[
            "INSERT INTO {t} VALUES (5,'gamma',50,true),(6,'ALPHA',60,false)",
            "UPDATE {t} SET k = 'beta', n = 25, active = false WHERE id = 1",
            "UPDATE {t} SET active = true WHERE id = 2",
            "DELETE FROM {t} WHERE id IN (3, 5)",
        ],
        rolled_back: &[
            "INSERT INTO {t} VALUES (90,'Alpha',20,true)",
            "UPDATE {t} SET k = 'zzz' WHERE id = 2",
            "DELETE FROM {t} WHERE id = 4",
        ],
        explain: "SELECT id FROM {t} WHERE lower(k) = 'alpha'",
        index_marks: &["_e"],
        plan_names_index: true,
    };
    // Act / Assert
    run_case(&case);
}

#[test]
fn should_enforce_and_maintain_unique_indexes_on_column_tables() {
    // Arrange
    with_pair(|pair| {
        pair.create("ux", "id INT, email TEXT", "(1,'a'),(2,'b'),(3,NULL)");
        for mode in [ROW, COL] {
            pair.run(&format!(
                "CREATE UNIQUE INDEX ux_{mode}_e ON ux_{mode} (email)"
            ));
        }
        // Act
        let mut states = Vec::new();
        for mode in [ROW, COL] {
            let table = format!("ux_{mode}");
            states.push(pair.sqlstate(&format!("INSERT INTO {table} VALUES (4,'a')")));
            pair.run(&format!("INSERT INTO {table} VALUES (5,NULL)"));
            pair.run(&format!("DELETE FROM {table} WHERE id = 1"));
            pair.run(&format!("INSERT INTO {table} VALUES (6,'a')"));
            pair.run(&format!("UPDATE {table} SET email = 'c' WHERE id = 6"));
            pair.run(&format!("INSERT INTO {table} VALUES (7,'a')"));
            states.push(pair.sqlstate(&format!("UPDATE {table} SET email = 'b' WHERE id = 7")));
        }
        // Assert
        assert_eq!(states[0], "23505");
        assert_eq!(states[..2], states[2..]);
        assert_eq!(
            pair.sorted("SELECT * FROM ux_col"),
            pair.sorted("SELECT * FROM ux_row")
        );
    });
}

#[test]
fn should_maintain_fulltext_indexes_on_column_tables() {
    // Arrange
    let case = Case {
        columns: "id INT, body TEXT",
        seed: "(1,'alpha beta'),(2,'alpha alpha gamma'),(3,'delta'),(4,NULL)",
        index_ddl: &["CREATE INDEX {t}_f ON {t} USING fulltext (body)"],
        queries: &[
            "SELECT id FROM {t} WHERE search(body,'alpha')",
            "SELECT id FROM {t} WHERE search(body,'alpha beta')",
            "SELECT id, search_score(body,'alpha') AS s FROM {t} WHERE search(body,'alpha') ORDER BY s DESC, id",
            "SELECT id, search_score(body,'delta') AS s FROM {t} ORDER BY s DESC, id LIMIT 2",
        ],
        mutations: &[
            "INSERT INTO {t} VALUES (5,'alpha delta'),(6,'epsilon')",
            "UPDATE {t} SET body = 'beta only' WHERE id = 2",
            "UPDATE {t} SET body = 'alpha restored' WHERE id = 3",
            "DELETE FROM {t} WHERE id = 1",
        ],
        rolled_back: &[
            "INSERT INTO {t} VALUES (90,'alpha alpha alpha')",
            "UPDATE {t} SET body = 'gone' WHERE id = 5",
            "DELETE FROM {t} WHERE id = 6",
        ],
        explain: "SELECT id FROM {t} WHERE search(body,'alpha')",
        index_marks: &["_f"],
        plan_names_index: false,
    };
    // Act / Assert
    run_case(&case);
}

const VECTOR_COLUMNS: &str = "id INT, emb VECTOR(3)";
const VECTOR_SEED: &str = "(1,'[1,0,0]'::vector),(2,'[0,1,0]'::vector),(3,'[-1,0,0]'::vector),(4,'[0.9,0.1,0]'::vector),(5,NULL)";
const VECTOR_QUERIES: &[&str] = &[
    "SELECT id FROM {t} ORDER BY emb <-> '[1,0,0]'::vector, id LIMIT 3",
    "SELECT id, vector_distance(emb, '[0,1,0]') AS d FROM {t} WHERE emb IS NOT NULL ORDER BY d, id LIMIT 2",
];
const VECTOR_MUTATIONS: &[&str] = &[
    "INSERT INTO {t} VALUES (6,'[0.95,0.05,0]'::vector),(7,'[0,0,1]'::vector)",
    "UPDATE {t} SET emb = '[0,1,0.1]'::vector WHERE id = 3",
    "DELETE FROM {t} WHERE id IN (1, 7)",
];
const VECTOR_ROLLED_BACK: &[&str] = &[
    "INSERT INTO {t} VALUES (90,'[1,0,0]'::vector)",
    "UPDATE {t} SET emb = '[0,0,1]'::vector WHERE id = 4",
    "DELETE FROM {t} WHERE id = 2",
];

#[test]
fn should_maintain_hnsw_indexes_on_column_tables() {
    // Arrange
    let case = Case {
        columns: VECTOR_COLUMNS,
        seed: VECTOR_SEED,
        index_ddl: &["CREATE INDEX {t}_v ON {t} USING hnsw (emb vector_l2_ops) WITH (m = 4, ef_construction = 16)"],
        queries: VECTOR_QUERIES,
        mutations: VECTOR_MUTATIONS,
        rolled_back: VECTOR_ROLLED_BACK,
        explain: "SELECT id FROM {t} ORDER BY emb <-> '[1,0,0]'::vector LIMIT 3",
        index_marks: &["_v", "_f"],
        plan_names_index: false,
    };
    // Act / Assert
    run_case(&case);
}

#[test]
fn should_maintain_ivfflat_indexes_on_column_tables() {
    // Arrange
    let case = Case {
        columns: VECTOR_COLUMNS,
        seed: VECTOR_SEED,
        index_ddl: &[
            "CREATE INDEX {t}_v ON {t} USING ivfflat (emb vector_l2_ops) WITH (lists = 1)",
        ],
        queries: VECTOR_QUERIES,
        mutations: VECTOR_MUTATIONS,
        rolled_back: VECTOR_ROLLED_BACK,
        explain: "SELECT id FROM {t} ORDER BY emb <-> '[1,0,0]'::vector LIMIT 3",
        index_marks: &["_v", "_f"],
        plan_names_index: false,
    };
    // Act / Assert
    run_case(&case);
}

#[test]
fn should_maintain_hybrid_retrieval_indexes_on_column_tables() {
    // Arrange
    let case = Case {
        columns: "id INT, body TEXT, emb VECTOR(3)",
        seed: "(1,'alpha beta','[1,0,0]'::vector),(2,'alpha','[0,1,0]'::vector),(3,'gamma','[0.9,0.1,0]'::vector),(4,'beta gamma','[0,0,1]'::vector)",
        index_ddl: &[
            "CREATE INDEX {t}_f ON {t} USING fulltext (body)",
            "CREATE INDEX {t}_v ON {t} USING hnsw (emb vector_l2_ops) WITH (m = 4, ef_construction = 16)",
        ],
        queries: &[
            "SELECT id, hybrid_score(search_score(body,'alpha'), vector_score(emb,'[1,0,0]')) AS hs FROM {t} ORDER BY hs DESC, id LIMIT 3",
        ],
        mutations: &[
            "INSERT INTO {t} VALUES (5,'alpha alpha','[0.8,0.2,0]'::vector)",
            "UPDATE {t} SET body = 'alpha gamma', emb = '[1,0.1,0]'::vector WHERE id = 3",
            "DELETE FROM {t} WHERE id = 1",
        ],
        rolled_back: &["INSERT INTO {t} VALUES (90,'alpha','[1,0,0]'::vector)"],
        explain: "SELECT id, hybrid_score(search_score(body,'alpha'), vector_score(emb,'[1,0,0]')) AS hs FROM {t} ORDER BY hs DESC LIMIT 3",
        index_marks: &["_v", "_f"],
        plan_names_index: false,
    };
    // Act / Assert
    run_case(&case);
}

#[test]
fn should_maintain_time_series_indexes_on_column_tables() {
    // Arrange
    let case = Case {
        columns: "tenant TEXT, event_at TIMESTAMP, amount INT",
        seed: "('a','2026-01-01T12:00:00Z',10),('a','2026-01-01T13:00:00Z',20),('b','2026-01-01T12:30:00Z',30),('a','2026-01-02T01:00:00Z',40)",
        index_ddl: &["CREATE INDEX {t}_ts ON {t} USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)"],
        queries: &[
            "SELECT tenant, event_at, amount FROM {t} WHERE tenant = 'a' AND event_at >= '2026-01-01T12:00:00Z' ORDER BY event_at",
            "SELECT tenant, amount FROM {t} WHERE event_at >= '2026-01-01T12:15:00Z' AND event_at < '2026-01-01T14:00:00Z' ORDER BY amount",
            "SELECT time_bucket('1 hour', event_at) AS bucket, count(*) FROM {t} GROUP BY time_bucket('1 hour', event_at) ORDER BY bucket",
        ],
        mutations: &[
            "INSERT INTO {t} VALUES ('b','2026-01-01T13:30:00Z',50),('a','2026-01-01T12:45:00Z',60)",
            "UPDATE {t} SET event_at = '2026-01-03T00:00:00Z' WHERE amount = 20",
            "DELETE FROM {t} WHERE amount = 10",
        ],
        rolled_back: &[
            "INSERT INTO {t} VALUES ('a','2026-01-01T12:10:00Z',99)",
            "DELETE FROM {t} WHERE amount = 30",
        ],
        explain: "SELECT tenant FROM {t} WHERE event_at >= '2026-01-01T00:00:00Z'",
        index_marks: &["_ts"],
        plan_names_index: true,
    };
    // Act / Assert
    run_case(&case);
}

#[test]
fn should_maintain_graph_indexes_on_column_backing_tables() {
    // Arrange
    with_pair(|pair| {
        // Backing tables of `CREATE GRAPH` have no selector; convert them
        // with the lane-A `SET ACCESS METHOD` statement.
        for graph in ["gr", "gc"] {
            pair.run(&format!("CREATE GRAPH {graph}"));
        }
        for table in ["gc_nodes", "gc_edges"] {
            pair.run(&format!("ALTER TABLE {table} SET ACCESS METHOD columnar"));
        }
        let edge = |graph: &str, id: &str, from: &str, to: &str| {
            format!(
                "INSERT INTO {graph}_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) \
                 VALUES ('{id}','n','{from}','n','{to}','r',1)"
            )
        };
        let neighbors = |graph: &str, node: &str, direction: &str| {
            pair.sorted(&format!(
                "SELECT node_id FROM graph_neighbors('{graph}','n','{node}','{direction}','*',10)"
            ))
        };
        let check = |stage: &str| {
            for (node, direction) in [("A", "out"), ("B", "out"), ("B", "in"), ("C", "in")] {
                assert_eq!(
                    neighbors("gc", node, direction),
                    neighbors("gr", node, direction),
                    "{stage}: {node} {direction}"
                );
            }
        };
        // Act / Assert
        for graph in ["gr", "gc"] {
            for (id, from, to) in [("e1", "A", "B"), ("e2", "A", "C"), ("e3", "B", "C")] {
                pair.run(&edge(graph, id, from, to));
            }
        }
        check("build");
        for graph in ["gr", "gc"] {
            pair.run(&edge(graph, "e4", "C", "A"));
            pair.run(&format!("DELETE FROM {graph}_edges WHERE edge_id = 'e2'"));
            pair.run(&format!(
                "UPDATE {graph}_edges SET target_id = 'A' WHERE edge_id = 'e3'"
            ));
        }
        check("mutation");
        for graph in ["gr", "gc"] {
            pair.run("BEGIN");
            pair.run(&edge(graph, "e9", "B", "A"));
            pair.run(&format!("DELETE FROM {graph}_edges WHERE edge_id = 'e1'"));
            pair.run("ROLLBACK");
        }
        check("rollback");
        assert!(!neighbors("gc", "A", "out").is_empty());
    });
}

#[test]
fn should_reject_column_indexes_on_column_tables_with_feature_not_supported() {
    // Arrange
    with_pair(|pair| {
        pair.create("cx", "id INT, label TEXT", "(1,'a'),(2,'b')");
        // Act
        let on_column_table =
            pair.sqlstate("CREATE INDEX cx_col_idx ON cx_col USING column (label)");
        let if_not_exists =
            pair.sqlstate("CREATE INDEX IF NOT EXISTS cx_col_idx ON cx_col USING column (label)");
        // Assert: row tables keep their derived column accelerator.
        pair.run("CREATE INDEX cx_row_idx ON cx_row USING column (label)");
        assert_eq!(on_column_table, "0A000");
        assert_eq!(if_not_exists, "0A000");
        // Nothing was published: the name is free and the data is intact.
        pair.run("CREATE INDEX cx_col_idx ON cx_col (label)");
        assert_eq!(
            pair.sorted("SELECT * FROM cx_col"),
            pair.sorted("SELECT * FROM cx_row")
        );
    });
}
