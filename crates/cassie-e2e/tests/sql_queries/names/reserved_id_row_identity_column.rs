use super::support_sql as support;

use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use support::data_dir;

fn run(cassie: &Cassie, session: &CassieSession, statements: &[&str]) {
    for statement in statements {
        cassie
            .execute_sql(session, statement, vec![])
            .expect(statement);
    }
}

fn metric(metrics: &serde_json::Value, key: &str) -> u64 {
    metrics["read_paths"][key].as_u64().unwrap_or(0)
}

fn seed_underscore_tables(label: &str) -> (String, Cassie, CassieSession) {
    let path = data_dir(label);
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    run(
        &cassie,
        &session,
        &[
            "CREATE TABLE underscore_declared (id INT, name TEXT)",
            "CREATE TABLE underscore_undeclared (name TEXT)",
            "INSERT INTO underscore_declared (id, name) VALUES (42, 'alice')",
            "INSERT INTO underscore_undeclared (name) VALUES ('bob')",
        ],
    );
    (path, cassie, session)
}

#[test]
fn should_select_underscore_id_beside_a_declared_id_column() {
    // Arrange
    let (path, cassie, session) = seed_underscore_tables("reserved_id_underscore_projection");
    let undeclared_identity = cassie
        .execute_sql(&session, "SELECT id FROM underscore_undeclared", vec![])
        .expect("bare id on undeclared table")
        .rows()[0][0]
        .clone();

    // Act
    let declared = cassie
        .execute_sql(
            &session,
            "SELECT _id, id, name FROM underscore_declared ORDER BY _id",
            vec![],
        )
        .expect("select _id beside a declared id");
    let undeclared = cassie
        .execute_sql(
            &session,
            "SELECT _id, name FROM underscore_undeclared ORDER BY _id",
            vec![],
        )
        .expect("select _id on an undeclared table");

    // Assert
    let declared_identity = declared.rows()[0][0].clone();
    assert!(
        matches!(&declared_identity, Value::String(identity) if !identity.is_empty()),
        "expected the internal identity string, got {declared_identity:?}"
    );
    assert_eq!(declared.rows()[0][1], Value::Int64(42));
    assert_eq!(declared.rows()[0][2], Value::String("alice".to_string()));
    assert_eq!(
        undeclared.rows(),
        vec![vec![undeclared_identity, Value::String("bob".to_string())]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_filter_a_declared_id_table_on_underscore_id() {
    // Arrange
    let (path, cassie, session) = seed_underscore_tables("reserved_id_underscore_filter");
    let identity = cassie
        .execute_sql(&session, "SELECT _id FROM underscore_declared", vec![])
        .expect("select the internal identity")
        .rows()[0][0]
        .clone();

    // Act
    let filtered = cassie
        .execute_sql(
            &session,
            "SELECT id FROM underscore_declared WHERE _id = $1",
            vec![identity],
        )
        .expect("filter on _id");

    // Assert
    assert_eq!(filtered.rows(), vec![vec![Value::Int64(42)]]);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_bare_id_against_a_derived_relation_that_does_not_project_it() {
    // Arrange
    let path = data_dir("reserved_id_derived_relation");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    run(
        &cassie,
        &session,
        &[
            "CREATE TABLE derived_source (name TEXT)",
            "INSERT INTO derived_source (name) VALUES ('alice')",
        ],
    );

    // Act
    let bare = cassie.execute_sql(
        &session,
        "SELECT id FROM (SELECT name FROM derived_source) s",
        vec![],
    );
    let reserved = cassie.execute_sql(
        &session,
        "SELECT _id FROM (SELECT name FROM derived_source) s",
        vec![],
    );

    // Assert
    let bare = bare
        .expect_err("a derived relation without id must reject a bare id")
        .to_string();
    assert!(
        bare.contains("unresolvable column reference 'id'"),
        "unexpected error: {bare}"
    );
    let reserved = reserved
        .expect_err("a derived relation must not synthesize the row identity")
        .to_string();
    assert!(
        reserved.contains("unresolvable column reference '_id'"),
        "unexpected error: {reserved}"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_use_scalar_index_order_with_underscore_id_tiebreak_on_declared_id_table() {
    // Arrange
    let path = data_dir("reserved_id_scalar_order_tiebreak");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    run(
        &cassie,
        &session,
        &[
            "CREATE TABLE tiebreak_docs (id TEXT, title TEXT, score INT)",
            "CREATE INDEX tiebreak_docs_score_idx ON tiebreak_docs USING btree (score)",
        ],
    );
    for index in 0..12_i64 {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO tiebreak_docs (id, title, score) VALUES ($1, $2, $3)",
                vec![
                    Value::String(format!("doc-{index}")),
                    Value::String(format!("title-{index}")),
                    Value::Int64(index % 3),
                ],
            )
            .expect("insert tiebreak row");
    }
    let mut expected = cassie
        .execute_sql(
            &session,
            "SELECT score, _id, id, title FROM tiebreak_docs WHERE score >= 1",
            vec![],
        )
        .expect("reference rows")
        .rows()
        .to_vec();
    expected.sort_by(|left, right| {
        let key = |row: &Vec<Value>| match (&row[0], &row[1]) {
            (Value::Int64(score), Value::String(identity)) => (*score, identity.clone()),
            other => panic!("unexpected reference row shape {other:?}"),
        };
        key(left).cmp(&key(right))
    });
    let expected = expected
        .into_iter()
        .take(5)
        .map(|row| vec![row[2].clone(), row[3].clone()])
        .collect::<Vec<_>>();
    let sql = "SELECT id, title FROM tiebreak_docs WHERE score >= $1 ORDER BY score, _id LIMIT 5";
    let before = cassie.metrics();

    // Act
    let result = cassie
        .execute_sql(&session, sql, vec![Value::Int64(1)])
        .expect("ordered tiebreak query");
    let after = cassie.metrics();

    // Assert
    assert!(
        metric(&after, "range_scans") + metric(&after, "ordered_bounded_scans")
            > metric(&before, "range_scans") + metric(&before, "ordered_bounded_scans"),
        "ORDER BY score, _id must use the scalar index: {}",
        after["read_paths"]
    );
    assert_eq!(result.rows(), expected);

    let _ = std::fs::remove_dir_all(path);
}
