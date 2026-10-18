use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::types::Value;

use super::support_sql as support;
use support::{data_dir, use_local_storage};

fn start(label: &str, statements: &[&str]) -> (Cassie, CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in statements {
        run(&cassie, &session, sql);
    }
    (cassie, session, path)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("{sql}: {error}"));
}

fn violated_constraint(
    result: Result<cassie::executor::QueryResult, CassieError>,
) -> Option<String> {
    if let Err(CassieError::UniqueViolation { constraint, .. }) = result {
        Some(constraint)
    } else {
        None
    }
}

fn ids(cassie: &Cassie, session: &CassieSession, table: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            &format!("SELECT id FROM {table} ORDER BY id"),
            vec![],
        )
        .expect("select ids")
        .rows()
        .to_vec()
}

fn int_ids(values: &[i64]) -> Vec<Vec<Value>> {
    values.iter().map(|id| vec![Value::Int64(*id)]).collect()
}

#[test]
fn should_reject_only_whole_tuple_duplicates_of_a_composite_unique_constraint() {
    // Arrange
    let (cassie, session, path) = start(
        "composite-unique-tuple",
        &[
            "CREATE TABLE f1 (id INT PRIMARY KEY, a INT, b INT, CONSTRAINT f1_ab UNIQUE (a, b))",
            "CREATE TABLE f2 (id INT PRIMARY KEY, a INT, b INT, UNIQUE (a, b))",
        ],
    );

    // Act
    let mut accepted = Vec::new();
    for table in ["f1", "f2"] {
        for sql in [
            "INSERT INTO {t} (id, a, b) VALUES (1, 10, 1)",
            "INSERT INTO {t} (id, a, b) VALUES (2, 10, 2)",
            "INSERT INTO {t} (id, a, b) VALUES (3, 11, 1)",
            "INSERT INTO {t} (id, a, b) VALUES (4, 10, NULL)",
            "INSERT INTO {t} (id, a, b) VALUES (5, 10, NULL)",
        ] {
            accepted.push(
                cassie
                    .execute_sql(&session, &sql.replace("{t}", table), vec![])
                    .is_ok(),
            );
        }
    }
    let named_duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO f1 (id, a, b) VALUES (6, 10, 1)",
        vec![],
    );
    let unnamed_duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO f2 (id, a, b) VALUES (6, 11, 1)",
        vec![],
    );
    let update_duplicate = cassie.execute_sql(&session, "UPDATE f1 SET b = 2 WHERE id = 1", vec![]);
    let unnamed_constraint = cassie
            .execute_sql(
                &session,
                "SELECT constraint_name FROM information_schema.table_constraints WHERE table_name = 'f2' AND constraint_type = 'UNIQUE'",
                vec![],
            )
            .expect("table constraints")
            .rows().to_vec();

    // Assert
    assert!(accepted.iter().all(|ok| *ok), "partial overlaps are legal");
    assert_eq!(
        violated_constraint(named_duplicate).as_deref(),
        Some("f1_ab")
    );
    assert_eq!(
        violated_constraint(unnamed_duplicate).as_deref(),
        Some("f2_a_b_key")
    );
    assert_eq!(
        violated_constraint(update_duplicate).as_deref(),
        Some("f1_ab")
    );
    assert_eq!(
        unnamed_constraint,
        vec![vec![Value::String("f2_a_b_key".to_string())]]
    );
    assert_eq!(ids(&cassie, &session, "f1"), int_ids(&[1, 2, 3, 4, 5]));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_manage_a_composite_unique_constraint_through_alter_table() {
    // Arrange
    let (cassie, session, path) = start(
        "composite-unique-alter",
        &[
            "CREATE TABLE g (id INT PRIMARY KEY, a INT, b INT)",
            "CREATE TABLE g_dup (id INT PRIMARY KEY, a INT, b INT)",
            "INSERT INTO g (id, a, b) VALUES (1, 10, 1), (2, 10, 2), (3, 11, 1)",
            "INSERT INTO g_dup (id, a, b) VALUES (1, 10, 1), (2, 10, 1)",
        ],
    );

    // Act
    let added = cassie.execute_sql(
        &session,
        "ALTER TABLE g ADD CONSTRAINT g_ab UNIQUE (a, b)",
        vec![],
    );
    let added_over_duplicate = cassie.execute_sql(
        &session,
        "ALTER TABLE g_dup ADD CONSTRAINT g_dup_ab UNIQUE (a, b)",
        vec![],
    );
    let partial_overlap = cassie.execute_sql(
        &session,
        "INSERT INTO g (id, a, b) VALUES (4, 11, 2)",
        vec![],
    );
    let duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO g (id, a, b) VALUES (5, 10, 2)",
        vec![],
    );
    run(&cassie, &session, "ALTER TABLE g DROP CONSTRAINT g_ab");
    let after_drop = cassie.execute_sql(
        &session,
        "INSERT INTO g (id, a, b) VALUES (6, 10, 2)",
        vec![],
    );

    // Assert
    assert!(added.is_ok(), "existing rows share only one column");
    assert!(added_over_duplicate.is_err(), "existing tuple duplicate");
    assert!(partial_overlap.is_ok(), "partial overlap is legal");
    assert_eq!(violated_constraint(duplicate).as_deref(), Some("g_ab"));
    assert!(after_drop.is_ok(), "the dropped constraint is released");
    assert_eq!(ids(&cassie, &session, "g"), int_ids(&[1, 2, 3, 4, 6]));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_keep_a_composite_unique_constraint_across_its_ddl_lifecycle() {
    // Arrange
    let (cassie, session, path) = start(
            "composite-unique-lifecycle",
            &[
                "CREATE TABLE k (id INT PRIMARY KEY, a INT, b INT, CONSTRAINT k_ab UNIQUE (a, b))",
                "CREATE TABLE k_drop (id INT PRIMARY KEY, a INT, b INT, CONSTRAINT k_drop_ab UNIQUE (a, b))",
                "INSERT INTO k (id, a, b) VALUES (1, 10, 1)",
                "ALTER TABLE k RENAME COLUMN a TO x",
            ],
        );

    // Act
    let renamed_duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO k (id, x, b) VALUES (2, 10, 1)",
        vec![],
    );
    let drop_backing_index = cassie.execute_sql(&session, "DROP INDEX k_ab ON k", vec![]);
    let other_column_in_second_constraint = cassie.execute_sql(
        &session,
        "ALTER TABLE k ADD CONSTRAINT k_bx UNIQUE (b, id)",
        vec![],
    );
    run(&cassie, &session, "ALTER TABLE k_drop DROP COLUMN a");
    let after_column_drop = cassie.execute_sql(
        &session,
        "INSERT INTO k_drop (id, b) VALUES (1, 1), (2, 1)",
        vec![],
    );
    let remaining_unique = cassie
            .execute_sql(
                &session,
                "SELECT constraint_name FROM information_schema.table_constraints WHERE table_name = 'k_drop' AND constraint_type = 'UNIQUE'",
                vec![],
            )
            .expect("table constraints")
            .rows().to_vec();
    drop(session);
    drop(cassie);
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen");
    reopened.startup().expect("restart");
    let session = reopened.create_session("tester", None);
    let restarted_duplicate = reopened.execute_sql(
        &session,
        "INSERT INTO k (id, x, b) VALUES (3, 10, 1)",
        vec![],
    );
    let restarted_partial = reopened.execute_sql(
        &session,
        "INSERT INTO k (id, x, b) VALUES (4, 10, 2)",
        vec![],
    );

    // Assert
    assert_eq!(
        violated_constraint(renamed_duplicate).as_deref(),
        Some("k_ab")
    );
    assert!(
        drop_backing_index.is_err(),
        "the constraint needs its index"
    );
    assert!(
        other_column_in_second_constraint.is_err(),
        "b already belongs to the multi-column constraint k_ab"
    );
    assert!(
        after_column_drop.is_ok(),
        "dropping a column drops k_drop_ab"
    );
    assert!(remaining_unique.is_empty(), "k_drop_ab is gone");
    assert_eq!(
        violated_constraint(restarted_duplicate).as_deref(),
        Some("k_ab")
    );
    assert!(restarted_partial.is_ok(), "partial overlap is legal");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_arbitrate_on_conflict_against_a_composite_unique_constraint() {
    // Arrange
    let (cassie, session, path) = start(
        "composite-unique-on-conflict",
        &[
            "CREATE TABLE h (id INT PRIMARY KEY, a INT, b INT, note TEXT, UNIQUE (a, b))",
            "INSERT INTO h (id, a, b, note) VALUES (1, 10, 1, 'first')",
        ],
    );

    // Act
    run(
        &cassie,
        &session,
        "INSERT INTO h (id, a, b, note) VALUES (2, 10, 2, 'second') ON CONFLICT DO NOTHING",
    );
    run(
        &cassie,
        &session,
        "INSERT INTO h (id, a, b, note) VALUES (3, 10, 1, 'ignored') ON CONFLICT (a, b) DO NOTHING",
    );
    run(
            &cassie,
            &session,
            "INSERT INTO h (id, a, b, note) VALUES (4, 10, 2, 'updated') ON CONFLICT (b, a) DO UPDATE SET note = excluded.note",
        );
    let notes = cassie
        .execute_sql(&session, "SELECT id, note FROM h ORDER BY id", vec![])
        .expect("select notes")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(
        notes,
        vec![
            vec![Value::Int64(1), Value::String("first".to_string())],
            vec![Value::Int64(2), Value::String("updated".to_string())],
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}
