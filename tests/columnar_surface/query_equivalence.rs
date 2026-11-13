//! Issue #1109: column tables answer every SELECT shape exactly as row tables.

use super::pair::{with_pair, with_pair_config, Pair};

const EMP_COLUMNS: &str = "id INT, g TEXT, v INT, s TEXT";
const EMP_ROWS: &str = "(1,'a',10,'x'),(2,'a',NULL,'y'),(3,'b',30,NULL),(4,'b',30,'z'),(5,NULL,50,'x'),(6,'c',NULL,NULL)";
const DIM_COLUMNS: &str = "k INT, label TEXT";
const DIM_ROWS: &str = "(1,'one'),(1,'uno'),(NULL,'nil'),(3,'three'),(9,'nine')";

fn seed(pair: &Pair) {
    pair.create("emp", EMP_COLUMNS, EMP_ROWS);
    pair.create("dim", DIM_COLUMNS, DIM_ROWS);
    pair.create("void", DIM_COLUMNS, "");
}

fn assert_all(pair: &Pair, tables: &[&str], queries: &[&str]) {
    for query in queries {
        pair.assert_equivalent(query, tables);
    }
}

#[test]
fn should_match_row_tables_for_filters_and_projections() {
    // Arrange
    with_pair(|pair| {
        seed(pair);
        // Act / Assert
        assert_all(
            pair,
            &["emp"],
            &[
                "SELECT id, v FROM {emp} WHERE v > 10",
                "SELECT id FROM {emp} WHERE v IS NULL",
                "SELECT id FROM {emp} WHERE v IS NOT NULL AND g = 'b'",
                "SELECT id FROM {emp} WHERE g IS NULL OR s IS NULL",
                "SELECT id FROM {emp} WHERE NOT (v = 30)",
                "SELECT id FROM {emp} WHERE v IN (10, 50)",
                "SELECT id FROM {emp} WHERE v NOT IN (10, 50)",
                "SELECT id FROM {emp} WHERE s LIKE 'x%' OR v BETWEEN 20 AND 40",
                "SELECT id, COALESCE(v, -1) AS cv, v + 1 AS nv, upper(s) AS us FROM {emp}",
                "SELECT id, CASE WHEN v IS NULL THEN 'none' WHEN v > 20 THEN 'big' ELSE 'small' END FROM {emp}",
                "SELECT * FROM {emp}",
                "SELECT _id IS NOT NULL FROM {emp}",
            ],
        );
    });
}

#[test]
fn should_match_row_tables_for_aggregates() {
    // Arrange
    with_pair(|pair| {
        seed(pair);
        // Act / Assert
        assert_all(
            pair,
            &["emp"],
            &[
                "SELECT count(*), count(v), sum(v), min(v), max(v), avg(v) FROM {emp}",
                "SELECT g, count(*), sum(v), count(DISTINCT v) FROM {emp} GROUP BY g",
                "SELECT g, sum(v) FROM {emp} GROUP BY g HAVING sum(v) > 20",
                "SELECT g, count(*) FROM {emp} GROUP BY g HAVING count(v) = 0",
                "SELECT DISTINCT g FROM {emp}",
                "SELECT DISTINCT g, v FROM {emp}",
                "SELECT g, s, count(*) FROM {emp} GROUP BY g, s",
                "SELECT count(*), sum(v) FROM {emp} WHERE id > 100",
                "SELECT g, count(*) FROM {emp} WHERE v IS NOT NULL GROUP BY g ORDER BY g",
            ],
        );
    });
}

#[test]
fn should_match_row_tables_for_ordering_and_pagination() {
    // Arrange
    with_pair(|pair| {
        seed(pair);
        // Act / Assert
        assert_all(
            pair,
            &["emp"],
            &[
                "SELECT id FROM {emp} ORDER BY id",
                "SELECT id FROM {emp} ORDER BY id DESC LIMIT 3",
                "SELECT id FROM {emp} ORDER BY v, id",
                "SELECT id FROM {emp} ORDER BY v DESC NULLS LAST, id",
                "SELECT id FROM {emp} ORDER BY g NULLS FIRST, id LIMIT 4 OFFSET 1",
                "SELECT id FROM {emp} ORDER BY id LIMIT 0",
                "SELECT id FROM {emp} ORDER BY id OFFSET 10",
                "SELECT id FROM {emp} WHERE v IS NOT NULL ORDER BY v, id LIMIT 2",
                "SELECT s, count(*) AS c FROM {emp} GROUP BY s ORDER BY c DESC, s NULLS LAST",
            ],
        );
    });
}

#[test]
fn should_match_row_tables_for_window_functions() {
    // Arrange
    with_pair(|pair| {
        seed(pair);
        // Act / Assert
        assert_all(
            pair,
            &["emp"],
            &[
                "SELECT id, row_number() OVER (ORDER BY id) FROM {emp} ORDER BY id",
                "SELECT id, rank() OVER (PARTITION BY g ORDER BY v) FROM {emp} ORDER BY id",
                "SELECT id, sum(v) OVER (PARTITION BY g) FROM {emp} ORDER BY id",
                "SELECT id, lag(v) OVER (ORDER BY id), lead(v) OVER (ORDER BY id) FROM {emp} ORDER BY id",
                "SELECT id, sum(v) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) FROM {emp} ORDER BY id",
                "SELECT id, count(*) OVER () FROM {emp} ORDER BY id",
            ],
        );
    });
}

#[test]
fn should_match_row_tables_for_ctes_and_subqueries() {
    // Arrange
    with_pair(|pair| {
        seed(pair);
        // Act / Assert
        assert_all(
            pair,
            &["emp", "dim"],
            &[
                "WITH big AS (SELECT id, v FROM {emp} WHERE v >= 30) SELECT id FROM big ORDER BY id",
                "WITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM r WHERE n < 6) \
                 SELECT e.id, r.n FROM {emp} e JOIN r ON r.n = e.id ORDER BY e.id",
                "WITH RECURSIVE r(n) AS (SELECT min(id) FROM {emp} UNION ALL SELECT n + 1 FROM r WHERE n < 4) \
                 SELECT n FROM r ORDER BY n",
                "SELECT id, (SELECT max(v) FROM {emp}) FROM {emp} ORDER BY id",
                "SELECT id FROM {emp} WHERE v = (SELECT min(v) FROM {emp})",
                "SELECT id FROM {emp} e WHERE EXISTS (SELECT 1 FROM {dim} d WHERE d.k = e.id)",
                "SELECT id FROM {emp} e WHERE NOT EXISTS (SELECT 1 FROM {dim} d WHERE d.k = e.id)",
                "SELECT id FROM {emp} WHERE id IN (SELECT k FROM {dim})",
                "SELECT id FROM {emp} WHERE id NOT IN (SELECT k FROM {dim} WHERE k IS NOT NULL)",
                "SELECT id FROM {emp} WHERE id NOT IN (SELECT k FROM {dim})",
                "SELECT t.g, t.total FROM (SELECT g, sum(v) AS total FROM {emp} GROUP BY g) t WHERE t.total > 0",
            ],
        );
    });
}

#[test]
fn should_match_row_tables_for_set_operations() {
    // Arrange
    with_pair(|pair| {
        seed(pair);
        // Act / Assert
        assert_all(
            pair,
            &["emp", "dim"],
            &[
                "SELECT id FROM {emp} UNION SELECT k FROM {dim}",
                "SELECT id FROM {emp} UNION ALL SELECT k FROM {dim}",
                "SELECT id FROM {emp} INTERSECT SELECT k FROM {dim}",
                "SELECT id FROM {emp} EXCEPT SELECT k FROM {dim}",
                "SELECT g FROM {emp} UNION SELECT label FROM {dim} ORDER BY 1",
                "SELECT k FROM {dim} EXCEPT ALL SELECT id FROM {emp}",
            ],
        );
    });
}

#[test]
fn should_match_row_tables_for_lateral_sources() {
    // Arrange
    with_pair(|pair| {
        seed(pair);
        // Act / Assert
        assert_all(
            pair,
            &["emp", "dim"],
            &[
                "SELECT e.id, l.m FROM {emp} e, LATERAL (SELECT max(d.k) AS m FROM {dim} d WHERE d.k <= e.id) l ORDER BY e.id",
                "SELECT e.id, l.label FROM {emp} e LEFT JOIN LATERAL (SELECT d.label FROM {dim} d WHERE d.k = e.id) l ON true ORDER BY e.id, l.label",
                "SELECT e.id, l.label FROM {emp} e CROSS JOIN LATERAL (SELECT d.label FROM {dim} d WHERE d.k = e.id ORDER BY d.label LIMIT 1) l ORDER BY e.id",
            ],
        );
    });
}

const JOIN_QUERIES: &[&str] = &[
    "SELECT e.id, d.label FROM {emp} e JOIN {dim} d ON e.id = d.k",
    "SELECT e.id, d.label FROM {emp} e LEFT JOIN {dim} d ON e.id = d.k",
    "SELECT e.id, d.label FROM {emp} e RIGHT JOIN {dim} d ON e.id = d.k",
    "SELECT e.id, d.label FROM {emp} e FULL JOIN {dim} d ON e.id = d.k",
    "SELECT e.id, d.k FROM {emp} e JOIN {dim} d ON e.v = d.k",
    "SELECT e.id, d.label FROM {emp} e JOIN {dim} d ON e.id = d.k AND e.v IS NULL",
    "SELECT e.id, d.label FROM {emp} e JOIN {dim} d ON e.id < d.k",
    "SELECT e.id, d.label FROM {emp} e CROSS JOIN {dim} d WHERE d.k IS NULL",
    "SELECT d.k, count(*) FROM {dim} d JOIN {emp} e ON d.k = e.id GROUP BY d.k",
    "SELECT e.id, d.label FROM {emp} e LEFT JOIN {void} d ON e.id = d.k",
    "SELECT e.id FROM {emp} e JOIN {void} d ON e.id = d.k",
    "SELECT d.label FROM {void} d FULL JOIN {dim} x ON d.k = x.k",
    "SELECT a.id, b.id FROM {emp} a JOIN {emp} b ON a.g = b.g AND a.id < b.id",
    "SELECT e.id, d.label FROM {emp} e JOIN {dim} d ON e.id = d.k ORDER BY e.id, d.label LIMIT 2",
];

fn assert_join_surface(pair: &Pair) {
    seed(pair);
    for query in JOIN_QUERIES {
        let tables: Vec<&str> = ["emp", "dim", "void"]
            .into_iter()
            .filter(|table| query.contains(&format!("{{{table}}}")))
            .collect();
        pair.assert_equivalent(query, &tables);
    }
}

#[test]
fn should_match_row_tables_for_joins_mixing_storage_modes() {
    // Arrange
    with_pair(assert_join_surface);
}

#[test]
fn should_match_row_tables_for_typed_batch_joins_and_parallel_scans() {
    // Arrange
    with_pair_config(
        |config| {
            config.limits.vectorized_joins_enabled = true;
            config.limits.parallel_scan_workers = 2;
        },
        // Act / Assert
        assert_join_surface,
    );
}

#[test]
fn should_match_row_tables_for_typed_scan_pipelines_with_many_rows() {
    // Arrange
    with_pair(|pair| {
        pair.create("bulk", "id INT, grp INT, val INT, tag TEXT", "");
        for mode in ["row", "col"] {
            let values: Vec<String> = (0..300)
                .map(|n| {
                    let val = if n % 7 == 0 {
                        "NULL".to_owned()
                    } else {
                        (n * 3 % 101).to_string()
                    };
                    format!("({n}, {}, {val}, 't{}')", n % 11, n % 5)
                })
                .collect();
            pair.run(&format!(
                "INSERT INTO bulk_{mode} VALUES {}",
                values.join(",")
            ));
        }
        // Act / Assert
        assert_all(
            pair,
            &["bulk"],
            &[
                "SELECT grp, count(*), sum(val), min(val), max(val) FROM {bulk} GROUP BY grp",
                "SELECT id FROM {bulk} WHERE val > 50 AND tag = 't2' ORDER BY id",
                "SELECT id, val FROM {bulk} ORDER BY val DESC NULLS LAST, id LIMIT 25 OFFSET 5",
                "SELECT tag, count(DISTINCT grp) FROM {bulk} GROUP BY tag HAVING count(*) > 10",
                "SELECT count(*) FROM {bulk} WHERE val IS NULL",
                "SELECT id, rank() OVER (PARTITION BY grp ORDER BY val) FROM {bulk} ORDER BY id",
            ],
        );
    });
}

#[test]
fn should_match_row_tables_after_update_delete_and_truncate() {
    // Arrange
    with_pair(|pair| {
        seed(pair);
        for mode in ["row", "col"] {
            pair.run(&format!("UPDATE emp_{mode} SET v = v + 1, s = 'u' WHERE g = 'b'"));
            pair.run(&format!("DELETE FROM emp_{mode} WHERE id = 1"));
            pair.run(&format!("INSERT INTO emp_{mode} VALUES (7,'d',NULL,NULL)"));
        }
        // Act / Assert
        assert_all(
            pair,
            &["emp"],
            &[
                "SELECT * FROM {emp} ORDER BY id",
                "SELECT g, count(*), sum(v) FROM {emp} GROUP BY g",
            ],
        );
        for mode in ["row", "col"] {
            pair.run(&format!("TRUNCATE emp_{mode}"));
        }
        assert_all(pair, &["emp"], &["SELECT count(*) FROM {emp}", "SELECT * FROM {emp}"]);
    });
}
