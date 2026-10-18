use super::support_sql as support;

use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use support::{data_dir, use_local_storage};

fn seeded(label: &str) -> (Cassie, CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE t (id INT, amount INT, name TEXT)",
        "INSERT INTO t (id, amount, name) VALUES (1, 5, 'ada'), (2, 7, 'bob')",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }
    (cassie, session, path)
}

#[test]
fn should_create_a_scalar_index_on_an_other_case_column() {
    // Arrange
    let (cassie, session, path) = seeded("index_case_scalar");

    // Act
    let created = cassie.execute_sql(&session, "CREATE INDEX t_idx ON t (AMOUNT)", vec![]);
    let filtered = cassie
        .execute_sql(&session, "SELECT name FROM t WHERE amount = 7", vec![])
        .expect("indexed filter");

    // Assert
    assert!(created.is_ok(), "CREATE INDEX on AMOUNT must succeed");
    assert_eq!(
        filtered.rows(),
        vec![vec![Value::String("bob".to_string())]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_not_use_a_scalar_range_index_on_a_case_distinct_sibling_column() {
    // Arrange
    use_local_storage();
    let path = data_dir("index_case_distinct_scalar_range");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE scalar_case_distinct (\"a\" INT, \"A\" INT)",
        "INSERT INTO scalar_case_distinct (\"a\", \"A\") VALUES \
             (1, 30), (20, 10), (30, 20), (40, 40)",
        "CREATE INDEX scalar_case_distinct_lower_idx ON scalar_case_distinct \
             USING btree (\"a\")",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }

    // Act
    let lower_plan = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT \"a\" FROM scalar_case_distinct \
                 WHERE \"a\" >= 20 ORDER BY \"a\"",
            vec![],
        )
        .expect("explain indexed lower field");
    let sibling = cassie
        .execute_sql(
            &session,
            "SELECT \"A\" FROM scalar_case_distinct \
                 WHERE \"A\" >= 20 ORDER BY \"A\"",
            vec![],
        )
        .expect("query case-distinct sibling field");
    let sibling_plan = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT \"A\" FROM scalar_case_distinct \
                 WHERE \"A\" >= 20 ORDER BY \"A\"",
            vec![],
        )
        .expect("explain case-distinct sibling field");

    // Assert
    let Value::String(lower_plan) = &lower_plan.rows()[0][0] else {
        panic!("expected textual lower-field plan");
    };
    let Value::String(sibling_plan) = &sibling_plan.rows()[0][0] else {
        panic!("expected textual sibling-field plan");
    };
    assert!(lower_plan.contains("scalar_case_distinct_lower_idx"));
    assert_eq!(
        sibling.rows(),
        vec![
            vec![Value::Int64(20)],
            vec![Value::Int64(30)],
            vec![Value::Int64(40)],
        ]
    );
    assert!(
        !sibling_plan.contains("scalar_case_distinct_lower_idx"),
        "the index on \"a\" must not serve \"A\""
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_create_a_unique_index_on_an_other_case_column() {
    // Arrange
    let (cassie, session, path) = seeded("index_case_unique");

    // Act
    let created = cassie.execute_sql(&session, "CREATE UNIQUE INDEX t_uidx ON t (NAME)", vec![]);
    let duplicate = cassie.execute_sql(
        &session,
        "INSERT INTO t (id, amount, name) VALUES (3, 9, 'ada')",
        vec![],
    );

    // Assert
    assert!(created.is_ok(), "CREATE UNIQUE INDEX on NAME must succeed");
    assert!(
        duplicate.is_err(),
        "the unique index must reject a repeated name"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bind_case_distinct_unique_indexes_to_their_exact_columns() {
    // Arrange
    use_local_storage();
    let path = data_dir("index_case_distinct_unique");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE unique_case_distinct (\"a\" TEXT, \"A\" TEXT)",
        "CREATE UNIQUE INDEX unique_case_distinct_lower ON unique_case_distinct (\"a\")",
        "CREATE UNIQUE INDEX unique_case_distinct_upper ON unique_case_distinct (\"A\")",
        "INSERT INTO unique_case_distinct (\"a\", \"A\") VALUES \
             ('shared', 'lower-only'), ('upper-only', 'shared')",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }

    // Act
    let duplicate_lower = cassie.execute_sql(
        &session,
        "INSERT INTO unique_case_distinct (\"a\", \"A\") \
             VALUES ('shared', 'independent')",
        vec![],
    );
    let duplicate_upper = cassie.execute_sql(
        &session,
        "INSERT INTO unique_case_distinct (\"a\", \"A\") \
             VALUES ('independent', 'shared')",
        vec![],
    );

    // Assert
    assert!(
        duplicate_lower.is_err(),
        "the unique index on \"a\" must reject duplicates"
    );
    assert!(
        duplicate_upper.is_err(),
        "the unique index on \"A\" must reject duplicates"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_create_an_index_with_other_case_include_columns() {
    // Arrange
    let (cassie, session, path) = seeded("index_case_include");

    // Act
    let created = cassie.execute_sql(
        &session,
        "CREATE INDEX t_inc ON t (Amount) INCLUDE (NAME)",
        vec![],
    );

    // Assert
    assert!(created.is_ok(), "INCLUDE on NAME must succeed");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_create_an_expression_index_over_an_other_case_column() {
    // Arrange
    let (cassie, session, path) = seeded("index_case_expression");

    // Act
    let created = cassie.execute_sql(&session, "CREATE INDEX t_e ON t ((lower(NAME)))", vec![]);
    let filtered = cassie
        .execute_sql(
            &session,
            "SELECT amount FROM t WHERE lower(name) = 'bob'",
            vec![],
        )
        .expect("expression filter");

    // Assert
    assert!(created.is_ok(), "expression index over NAME must succeed");
    assert_eq!(filtered.rows(), vec![vec![Value::Int64(7)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_not_use_an_expression_index_on_a_case_distinct_sibling_column() {
    // Arrange
    use_local_storage();
    let path = data_dir("index_case_distinct_expression");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
            "CREATE TABLE expression_case_distinct (\"a\" TEXT, \"A\" TEXT)",
            "INSERT INTO expression_case_distinct (\"a\", \"A\") VALUES ('indexed', 'other'), ('miss', 'indexed')",
            "CREATE INDEX expression_case_distinct_lower_a ON expression_case_distinct USING btree (lower(\"a\"))",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| panic!("{statement}: {error}"));
        }

    // Act
    let indexed = cassie
        .execute_sql(
            &session,
            "SELECT \"a\" FROM expression_case_distinct WHERE lower(\"a\") = 'indexed'",
            vec![],
        )
        .expect("query indexed expression");
    let sibling = cassie
        .execute_sql(
            &session,
            "SELECT \"A\" FROM expression_case_distinct WHERE lower(\"A\") = 'indexed'",
            vec![],
        )
        .expect("query case-distinct sibling expression");
    let sibling_plan = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT \"A\" FROM expression_case_distinct WHERE lower(\"A\") = 'indexed'",
            vec![],
        )
        .expect("explain case-distinct sibling expression");

    // Assert
    assert_eq!(
        indexed.rows(),
        vec![vec![Value::String("indexed".to_string())]]
    );
    assert_eq!(
        sibling.rows(),
        vec![vec![Value::String("indexed".to_string())]]
    );
    let sibling_plan = sibling_plan
        .rows()
        .iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !sibling_plan.contains("expression_case_distinct_lower_a"),
        "expression index on \"a\" must not serve \"A\": {sibling_plan}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_create_a_fulltext_index_on_an_other_case_column() {
    // Arrange
    let (cassie, session, path) = seeded("index_case_fulltext");

    // Act
    let created = cassie.execute_sql(
        &session,
        "CREATE INDEX t_ft ON t USING fulltext (NAME)",
        vec![],
    );

    // Assert
    assert!(created.is_ok(), "fulltext index on NAME must succeed");
    let _ = std::fs::remove_dir_all(path);
}
