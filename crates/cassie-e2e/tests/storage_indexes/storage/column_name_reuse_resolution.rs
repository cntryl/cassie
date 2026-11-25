use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use super::support_sql as support;

fn open(label: &str, statements: &[&str]) -> (Cassie, CassieSession, String) {
    let path = support::data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in statements {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }
    (cassie, session, path)
}

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect(sql)
        .rows()
        .to_vec()
}

#[test]
fn should_read_null_for_readded_column_on_every_read_path() {
    // Arrange
    let (cassie, session, path) = open(
        "readded_same_type_column",
        &[
            "CREATE TABLE readded_same_type (pk INT PRIMARY KEY, a INT)",
            "INSERT INTO readded_same_type (pk, a) VALUES (1, 100)",
            "ALTER TABLE readded_same_type DROP COLUMN a",
            "ALTER TABLE readded_same_type ADD COLUMN a INT",
        ],
    );

    // Act
    let projected = rows(&cassie, &session, "SELECT pk, a FROM readded_same_type");
    let matched = rows(
        &cassie,
        &session,
        "SELECT pk FROM readded_same_type WHERE a = 100",
    );
    let null_rows = rows(
        &cassie,
        &session,
        "SELECT pk FROM readded_same_type WHERE a IS NULL",
    );
    let counted = rows(&cassie, &session, "SELECT COUNT(a) FROM readded_same_type");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX readded_same_type_a_idx ON readded_same_type (a)",
            vec![],
        )
        .expect("index re-added column");
    let indexed = rows(
        &cassie,
        &session,
        "SELECT pk FROM readded_same_type WHERE a = 100",
    );

    // Assert
    assert_eq!(projected, vec![vec![Value::Int64(1), Value::Null]]);
    assert!(
        matched.is_empty(),
        "retired value must not satisfy a filter"
    );
    assert_eq!(null_rows, vec![vec![Value::Int64(1)]]);
    assert_eq!(counted, vec![vec![Value::Int64(0)]]);
    assert!(indexed.is_empty(), "index and scan must agree");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resolve_a_name_moved_between_columns_to_its_live_owner() {
    // Arrange
    let (cassie, session, path) = open(
        "rename_name_moved_between_columns",
        &[
            "CREATE TABLE rename_swap (a INT, b INT)",
            "INSERT INTO rename_swap (a, b) VALUES (1, 2)",
            "ALTER TABLE rename_swap RENAME COLUMN b TO z",
            "ALTER TABLE rename_swap RENAME COLUMN a TO b",
            "INSERT INTO rename_swap (b, z) VALUES (10, 20)",
        ],
    );

    // Act
    let projected = rows(&cassie, &session, "SELECT b, z FROM rename_swap ORDER BY z");
    let only_b = rows(&cassie, &session, "SELECT b FROM rename_swap ORDER BY b");
    let filtered = rows(&cassie, &session, "SELECT z FROM rename_swap WHERE b = 10");
    let misfiltered = rows(&cassie, &session, "SELECT z FROM rename_swap WHERE b = 20");

    // Assert
    assert_eq!(
        projected,
        vec![
            vec![Value::Int64(1), Value::Int64(2)],
            vec![Value::Int64(10), Value::Int64(20)],
        ]
    );
    assert_eq!(only_b, vec![vec![Value::Int64(1)], vec![Value::Int64(10)]]);
    assert_eq!(filtered, vec![vec![Value::Int64(20)]]);
    assert!(misfiltered.is_empty(), "filter must apply to b, not z");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_emit_declared_type_for_column_readded_with_a_different_type() {
    // Arrange
    let (cassie, session, path) = open(
            "readded_different_type_column",
            &[
                "CREATE TABLE readded_other_type (id TEXT PRIMARY KEY, note TEXT, old_name TEXT)",
                "INSERT INTO readded_other_type (id, note, old_name) VALUES ('r1', 'secret', 'VALUE')",
                "ALTER TABLE readded_other_type DROP COLUMN note",
                "ALTER TABLE readded_other_type ADD COLUMN note INT",
                "ALTER TABLE readded_other_type RENAME COLUMN old_name TO new_name",
                "ALTER TABLE readded_other_type ADD COLUMN old_name INT",
                "INSERT INTO readded_other_type (id, new_name, old_name, note) VALUES ('r2', 'V2', 7, 8)",
            ],
        );

    // Act
    let projected = rows(
        &cassie,
        &session,
        "SELECT id, note, new_name, old_name FROM readded_other_type ORDER BY id",
    );
    let not_null = rows(
            &cassie,
            &session,
            "SELECT id FROM readded_other_type WHERE note IS NOT NULL OR old_name IS NOT NULL ORDER BY id",
        );

    // Assert
    assert_eq!(
        projected,
        vec![
            vec![
                Value::String("r1".to_string()),
                Value::Null,
                Value::String("VALUE".to_string()),
                Value::Null,
            ],
            vec![
                Value::String("r2".to_string()),
                Value::Int64(8),
                Value::String("V2".to_string()),
                Value::Int64(7),
            ],
        ]
    );
    assert_eq!(not_null, vec![vec![Value::String("r2".to_string())]]);
    let _ = std::fs::remove_dir_all(path);
}
