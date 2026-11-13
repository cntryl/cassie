//! Issue #1110: column tables enforce constraints with the semantics and
//! SQLSTATEs of row tables, as either side of a foreign key.

use super::pair::{with_pair, Pair, COL, ROW};
use cassie::app::Cassie;
use cassie::types::Value;

fn snapshot(pair: &Pair, table: &str) -> Vec<Vec<Value>> {
    pair.sorted(&format!("SELECT * FROM {table}"))
}

/// Creates the table in both modes, then proves each failing statement raises
/// `expected` (when given) in both modes, the same SQLSTATE across modes, and
/// leaves the table unchanged.
fn assert_rejected(
    pair: &Pair,
    name: &str,
    columns: &str,
    seed: &str,
    failing: &[&str],
    expected: Option<&str>,
) {
    pair.create(name, columns, seed);
    for statement in failing {
        let mut states = Vec::new();
        for mode in [ROW, COL] {
            let table = format!("{name}_{mode}");
            let before = snapshot(pair, &table);
            let state = pair.sqlstate(&Pair::resolved(statement, name, mode));
            assert_eq!(snapshot(pair, &table), before, "{statement} changed {mode}");
            states.push(state);
        }
        assert_eq!(states[0], states[1], "{statement}");
        if let Some(expected) = expected {
            assert_eq!(states[1], expected, "{statement}");
        }
    }
}

#[test]
fn should_reject_not_null_violations_like_row_tables() {
    // Arrange
    with_pair(|pair| {
        // Act / Assert
        assert_rejected(
            pair,
            "nn",
            "id INT PRIMARY KEY, v INT NOT NULL",
            "(1, 10)",
            &[
                "INSERT INTO {nn} VALUES (2, NULL)",
                "UPDATE {nn} SET v = NULL WHERE id = 1",
                "INSERT INTO {nn} (id) VALUES (3)",
            ],
            Some("23502"),
        );
    });
}

#[test]
fn should_reject_check_violations_like_row_tables() {
    // Arrange
    with_pair(|pair| {
        // Act / Assert
        assert_rejected(
            pair,
            "ck",
            "id INT PRIMARY KEY, qty INT CHECK (qty > 0), CONSTRAINT lo_hi CHECK (qty < 1000)",
            "(1, 5)",
            &[
                "INSERT INTO {ck} VALUES (2, 0)",
                "INSERT INTO {ck} VALUES (3, 5000)",
                "UPDATE {ck} SET qty = -1 WHERE id = 1",
            ],
            Some("23514"),
        );
        // NULL satisfies CHECK in both modes.
        for mode in [ROW, COL] {
            pair.run(&format!("INSERT INTO ck_{mode} VALUES (9, NULL)"));
        }
        assert_eq!(snapshot(pair, "ck_row"), snapshot(pair, "ck_col"));
    });
}

#[test]
fn should_reject_key_and_unique_violations_like_row_tables() {
    // Arrange
    with_pair(|pair| {
        // Act / Assert
        assert_rejected(
            pair,
            "uq",
            "id INT PRIMARY KEY, email TEXT UNIQUE, a INT, b INT, UNIQUE (a, b)",
            "(1, 'x@y', 1, 1), (2, NULL, 2, NULL)",
            &[
                "INSERT INTO {uq} VALUES (1, 'z@y', 7, 7)",
                "INSERT INTO {uq} VALUES (3, 'x@y', 7, 7)",
                "INSERT INTO {uq} VALUES (4, 'q@y', 1, 1)",
                "UPDATE {uq} SET id = 1 WHERE id = 2",
                "UPDATE {uq} SET email = 'x@y' WHERE id = 2",
                "INSERT INTO {uq} VALUES (5, 'n@y', 8, 8), (6, 'n@y', 9, 9)",
                "INSERT INTO {uq} VALUES (NULL, 'n2@y', 8, 8)",
            ],
            None,
        );
        // NULLs never conflict with each other in UNIQUE columns.
        for mode in [ROW, COL] {
            pair.run(&format!("INSERT INTO uq_{mode} VALUES (10, NULL, 3, NULL), (11, NULL, 3, NULL)"));
        }
        assert_eq!(snapshot(pair, "uq_row"), snapshot(pair, "uq_col"));
        assert_eq!(pair.sqlstate("INSERT INTO uq_col VALUES (1, 'a@y', 50, 50)"), "23505");
    });
}

#[test]
fn should_apply_on_conflict_like_row_tables() {
    // Arrange
    with_pair(|pair| {
        pair.create("oc", "id INT PRIMARY KEY, email TEXT UNIQUE, n INT", "(1,'a',1),(2,'b',2)");
        for mode in [ROW, COL] {
            // Act
            pair.run(&format!("INSERT INTO oc_{mode} VALUES (1,'zz',9) ON CONFLICT (id) DO NOTHING"));
            pair.run(&format!(
                "INSERT INTO oc_{mode} VALUES (2,'b',20) ON CONFLICT (id) DO UPDATE SET n = excluded.n + oc_{mode}.n"
            ));
            pair.run(&format!(
                "INSERT INTO oc_{mode} VALUES (3,'a',30) ON CONFLICT (email) DO UPDATE SET n = excluded.n"
            ));
            pair.run(&format!("INSERT INTO oc_{mode} VALUES (4,'d',4) ON CONFLICT DO NOTHING"));
        }
        // Assert
        assert_eq!(snapshot(pair, "oc_col"), snapshot(pair, "oc_row"));
        assert_eq!(pair.run("SELECT id FROM oc_col").len(), 3);
        assert_eq!(
            pair.sqlstate("INSERT INTO oc_col VALUES (5,'a',1) ON CONFLICT (id) DO NOTHING"),
            pair.sqlstate("INSERT INTO oc_row VALUES (5,'a',1) ON CONFLICT (id) DO NOTHING")
        );
    });
}

#[test]
fn should_fill_defaults_identity_and_generated_columns_like_row_tables() {
    // Arrange
    with_pair(|pair| {
        pair.create(
            "gen",
            "id INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, label TEXT DEFAULT 'none', \
             qty INT DEFAULT 2 + 3, doubled INT GENERATED ALWAYS AS (qty * 2) STORED, \
             seq INT GENERATED BY DEFAULT AS IDENTITY",
            "",
        );
        for mode in [ROW, COL] {
            // Act
            pair.run(&format!("INSERT INTO gen_{mode} (label) VALUES ('a')"));
            pair.run(&format!("INSERT INTO gen_{mode} (qty, seq) VALUES (10, 100)"));
            pair.run(&format!("INSERT INTO gen_{mode} DEFAULT VALUES"));
            pair.run(&format!("UPDATE gen_{mode} SET qty = qty + 1 WHERE label = 'a'"));
        }
        // Assert
        assert_eq!(snapshot(pair, "gen_col"), snapshot(pair, "gen_row"));
        let doubled = pair.run("SELECT doubled FROM gen_col WHERE label = 'a'");
        assert_eq!(doubled.len(), 1);
        assert_eq!(doubled, pair.run("SELECT doubled FROM gen_row WHERE label = 'a'"));
        for statement in [
            "INSERT INTO {gen} (id, label) VALUES (99, 'x')",
            "INSERT INTO {gen} (doubled) VALUES (1)",
            "UPDATE {gen} SET doubled = 3",
            "UPDATE {gen} SET id = 50",
        ] {
            assert_eq!(
                pair.sqlstate(&Pair::resolved(statement, "gen", COL)),
                pair.sqlstate(&Pair::resolved(statement, "gen", ROW)),
                "{statement}"
            );
        }
        assert_eq!(snapshot(pair, "gen_col"), snapshot(pair, "gen_row"));
    });
}

#[test]
fn should_enforce_domains_on_column_tables_like_row_tables() {
    // Arrange
    with_pair(|pair| {
        pair.run("CREATE DOMAIN posint AS int NOT NULL CHECK (VALUE > 0)");
        pair.run("CREATE DOMAIN shortcode AS text CHECK (length(VALUE) <= 3)");
        // Act / Assert
        assert_rejected(
            pair,
            "dm",
            "id INT PRIMARY KEY, n posint, c shortcode",
            "(1, 5, 'ab')",
            &[
                "INSERT INTO {dm} VALUES (2, 0, 'ab')",
                "INSERT INTO {dm} VALUES (3, NULL, 'ab')",
                "INSERT INTO {dm} VALUES (4, 1, 'abcd')",
                "UPDATE {dm} SET n = -3 WHERE id = 1",
                "UPDATE {dm} SET c = 'toolong' WHERE id = 1",
            ],
            None,
        );
    });
}

struct Combo {
    parent: &'static str,
    child: &'static str,
}

const COMBOS: [Combo; 4] = [
    Combo { parent: ROW, child: ROW },
    Combo { parent: ROW, child: COL },
    Combo { parent: COL, child: ROW },
    Combo { parent: COL, child: COL },
];

fn storage(mode: &str) -> &'static str {
    if mode == COL {
        " WITH (storage = column_store)"
    } else {
        ""
    }
}

fn fk_fixture(pair: &Pair, index: usize, combo: &Combo, actions: &str) -> (String, String) {
    let parent = format!("fkp{index}");
    let child = format!("fkc{index}");
    pair.run(&format!(
        "CREATE TABLE {parent} (id INT PRIMARY KEY, code TEXT UNIQUE){}",
        storage(combo.parent)
    ));
    pair.run(&format!(
        "CREATE TABLE {child} (id INT PRIMARY KEY, pid INT, FOREIGN KEY (pid) REFERENCES {parent}(id) {actions}){}",
        storage(combo.child)
    ));
    pair.run(&format!("INSERT INTO {parent} VALUES (1,'a'),(2,'b'),(3,'c')"));
    pair.run(&format!("INSERT INTO {child} VALUES (10,1),(11,1),(12,2),(13,NULL)"));
    (parent, child)
}

#[test]
fn should_enforce_foreign_keys_across_storage_modes() {
    // Arrange
    with_pair(|pair| {
        let mut outcomes = Vec::new();
        for (index, combo) in COMBOS.iter().enumerate() {
            let (parent, child) = fk_fixture(pair, index, combo, "");
            // Act
            let states = [
                pair.sqlstate(&format!("INSERT INTO {child} VALUES (14, 99)")),
                pair.sqlstate(&format!("UPDATE {child} SET pid = 98 WHERE id = 10")),
                pair.sqlstate(&format!("DELETE FROM {parent} WHERE id = 1")),
                pair.sqlstate(&format!("UPDATE {parent} SET id = 50 WHERE id = 2")),
            ];
            pair.run(&format!("INSERT INTO {child} VALUES (15, 3)"));
            outcomes.push((
                states,
                snapshot(pair, &parent),
                snapshot(pair, &child),
            ));
        }
        // Assert
        for outcome in &outcomes {
            assert_eq!(outcome.0[0], "23503");
            assert_eq!(outcome.0[1], "23503");
            assert_eq!(outcome.0[2], "23503");
            assert_eq!(outcome.0[3], "23503");
            assert_eq!(outcome, &outcomes[0]);
        }
    });
}

#[test]
fn should_apply_foreign_key_actions_across_storage_modes() {
    // Arrange
    with_pair(|pair| {
        let mut outcomes = Vec::new();
        for (index, combo) in COMBOS.iter().enumerate() {
            let (parent, child) = fk_fixture(pair, index, combo, "ON DELETE CASCADE ON UPDATE CASCADE");
            // Act
            pair.run(&format!("DELETE FROM {parent} WHERE id = 1"));
            pair.run(&format!("UPDATE {parent} SET id = 20 WHERE id = 2"));
            outcomes.push((snapshot(pair, &parent), snapshot(pair, &child)));
        }
        for (index, combo) in COMBOS.iter().enumerate() {
            let (parent, child) = fk_fixture(pair, index + 10, combo, "ON DELETE SET NULL");
            pair.run(&format!("DELETE FROM {parent} WHERE id = 2"));
            outcomes.push((snapshot(pair, &parent), snapshot(pair, &child)));
        }
        // Assert
        for group in outcomes.chunks(4) {
            for outcome in group {
                assert_eq!(outcome, &group[0]);
            }
        }
        assert_eq!(pair.run("SELECT id FROM fkc3").len(), 2);
    });
}

#[test]
fn should_report_constraints_in_catalogs_like_row_tables() {
    // Arrange
    with_pair(|pair| {
        pair.create(
            "cat",
            "id INT PRIMARY KEY, email TEXT UNIQUE NOT NULL, qty INT CHECK (qty > 0)",
            "",
        );
        // Act
        let kinds = |mode: &str| {
            pair.run(&format!(
                "SELECT constraint_type FROM information_schema.table_constraints \
                 WHERE table_name = 'cat_{mode}' ORDER BY constraint_type"
            ))
        };
        let pg_kinds = |mode: &str| {
            pair.run(&format!(
                "SELECT c.contype::TEXT FROM pg_catalog.pg_constraint c \
                 JOIN pg_catalog.pg_class r ON r.relname = c.conrelid \
                 WHERE r.relname = 'cat_{mode}' ORDER BY 1"
            ))
        };
        let columns = |mode: &str| {
            pair.run(&format!(
                "SELECT column_name, is_nullable, data_type FROM information_schema.columns \
                 WHERE table_name = 'cat_{mode}' ORDER BY ordinal_position"
            ))
        };
        // Assert
        assert!(!kinds(ROW).is_empty());
        assert_eq!(kinds(COL), kinds(ROW));
        assert!(!pg_kinds(ROW).is_empty());
        assert_eq!(pg_kinds(COL), pg_kinds(ROW));
        assert_eq!(columns(COL), columns(ROW));
    });
}

#[test]
fn should_roll_back_column_writes_to_savepoints_like_row_tables() {
    // Arrange
    with_pair(|pair| {
        pair.create("sp", "id INT PRIMARY KEY, email TEXT UNIQUE, v INT CHECK (v >= 0)", "(1,'a',1)");
        for mode in [ROW, COL] {
            let t = format!("sp_{mode}");
            // Act
            pair.run("BEGIN");
            pair.run(&format!("INSERT INTO {t} VALUES (2,'b',2)"));
            pair.run("SAVEPOINT s1");
            pair.run(&format!("INSERT INTO {t} VALUES (3,'c',3)"));
            pair.run(&format!("UPDATE {t} SET v = 100 WHERE id = 1"));
            pair.run(&format!("DELETE FROM {t} WHERE id = 2"));
            assert_eq!(pair.sqlstate(&format!("INSERT INTO {t} VALUES (4,'a',4)")), "23505");
            pair.run("ROLLBACK TO SAVEPOINT s1");
            // The reservation for 'c' was released and 'b' is live again.
            pair.run(&format!("INSERT INTO {t} VALUES (5,'c',5)"));
            assert_eq!(pair.sqlstate(&format!("INSERT INTO {t} VALUES (6,'b',6)")), "23505");
            pair.run("ROLLBACK");
            // Assert: the whole transaction left nothing behind.
            assert_eq!(
                pair.run(&format!("SELECT id, email, v FROM {t} ORDER BY id")).len(),
                1,
                "{mode}"
            );
            pair.run(&format!("INSERT INTO {t} VALUES (2,'b',2)"));
            pair.run(&format!("DELETE FROM {t} WHERE id = 2"));
        }
        assert_eq!(snapshot(pair, "sp_col"), snapshot(pair, "sp_row"));
    });
}

#[test]
fn should_enforce_foreign_keys_after_restart_on_local_storage() {
    // Arrange
    cassie_test_support::support_sql::use_local_storage();
    let path = cassie_test_support::support_sql::data_dir("columnar_constraints_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let run = |cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str| {
            cassie
                .execute_sql(session, sql, vec![])
                .unwrap_or_else(|error| panic!("{sql}: {error}"))
        };
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE TABLE rp (id INT PRIMARY KEY, code TEXT UNIQUE NOT NULL, n INT CHECK (n > 0)) WITH (storage = column_store)",
            "CREATE TABLE rc (id INT PRIMARY KEY, pid INT REFERENCES rp(id) ON DELETE CASCADE)",
            "INSERT INTO rp VALUES (1,'a',1),(2,'b',2)",
            "INSERT INTO rc VALUES (10,1),(11,2)",
        ] {
            run(&cassie, &session, statement);
        }
        drop(cassie);
        let cassie = Cassie::new_with_data_dir(&path).expect("reopen");
        cassie.startup().expect("restart");
        let session = cassie.create_session("tester", None);
        // Act
        let states: Vec<String> = [
            "INSERT INTO rp VALUES (3,'a',3)",
            "INSERT INTO rp VALUES (3,'c',0)",
            "INSERT INTO rp VALUES (3,NULL,3)",
            "INSERT INTO rc VALUES (12, 99)",
        ]
        .iter()
        .map(|sql| {
            cassie
                .execute_sql(&session, sql, vec![])
                .err()
                .map_or_else(String::new, |error| error.sql_state().to_owned())
        })
        .collect();
        run(&cassie, &session, "DELETE FROM rp WHERE id = 1");
        let remaining = run(&cassie, &session, "SELECT id FROM rc ORDER BY id");
        // Assert
        assert_eq!(states, ["23505", "23514", "23502", "23503"]);
        assert_eq!(remaining.rows().len(), 1);
    });
    drop(runtime);
    let _ = std::fs::remove_dir_all(path);
}
