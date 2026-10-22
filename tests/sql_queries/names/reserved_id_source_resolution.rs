use super::support_sql as support;

use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use support::data_dir;

fn identity_of(cassie: &Cassie, session: &CassieSession, sql: &str) -> String {
    let result = cassie.execute_sql(session, sql, vec![]).expect("select id");
    match &result.rows()[0][0] {
        Value::String(id) => id.clone(),
        other => panic!("expected an internal identity string, got {other:?}"),
    }
}

#[test]
fn should_resolve_id_to_the_internal_identity_in_every_source_shape() {
    // Arrange
    let path = data_dir("reserved_id_undeclared_sources");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE reserved_id_left (name TEXT)",
        "CREATE TABLE reserved_id_right (name TEXT)",
        "CREATE TABLE reserved_id_sink (doc TEXT)",
        "INSERT INTO reserved_id_left (name) VALUES ('alice')",
        "INSERT INTO reserved_id_right (name) VALUES ('alice')",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }
    let identity = identity_of(&cassie, &session, "SELECT id FROM reserved_id_left");

    // Act
    let joined = cassie
            .execute_sql(
                &session,
                "SELECT id FROM reserved_id_left JOIN reserved_id_right ON reserved_id_left.name = reserved_id_right.name",
                vec![],
            )
            .expect("join");
    let derived = cassie
        .execute_sql(
            &session,
            "SELECT id FROM (SELECT id, name FROM reserved_id_left) s",
            vec![],
        )
        .expect("derived table");
    let cte = cassie
        .execute_sql(
            &session,
            "WITH c AS (SELECT * FROM reserved_id_left) SELECT id FROM c",
            vec![],
        )
        .expect("cte reference");
    let union = cassie
        .execute_sql(
            &session,
            "SELECT id FROM reserved_id_left UNION SELECT id FROM reserved_id_left",
            vec![],
        )
        .expect("set operation");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO reserved_id_sink (doc) SELECT id FROM reserved_id_left",
            vec![],
        )
        .expect("insert select");
    let inserted = cassie
        .execute_sql(&session, "SELECT doc FROM reserved_id_sink", vec![])
        .expect("read back insert select");

    // Assert
    let expected = vec![vec![Value::String(identity.clone())]];
    assert_eq!(joined.rows(), expected, "join lost the internal identity");
    assert_eq!(
        derived.rows(),
        expected,
        "derived table lost the internal identity"
    );
    assert_eq!(
        cte.rows(),
        expected,
        "CTE reference lost the internal identity"
    );
    assert_eq!(
        union.rows(),
        expected,
        "set operation lost the internal identity"
    );
    assert_eq!(
        inserted.rows(),
        expected,
        "INSERT ... SELECT stored NULL instead of the internal identity"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resolve_a_declared_id_column_in_every_source_shape() {
    // Arrange
    let path = data_dir("reserved_id_declared_sources");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE declared_id_rows (id INT, name TEXT)",
        "CREATE TABLE declared_id_other (id INT, note TEXT)",
        "INSERT INTO declared_id_rows (id, name) VALUES (42, 'alice')",
        "INSERT INTO declared_id_rows (id, name) VALUES (7, 'carol')",
        "INSERT INTO declared_id_other (id, note) VALUES (99, 'linked')",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }

    // Act
    let derived = cassie
        .execute_sql(
            &session,
            "SELECT id FROM (SELECT id, name FROM declared_id_rows) s ORDER BY id",
            vec![],
        )
        .expect("derived table");
    let cte = cassie
        .execute_sql(
            &session,
            "WITH c AS (SELECT * FROM declared_id_rows) SELECT id FROM c ORDER BY id",
            vec![],
        )
        .expect("cte reference");
    let union = cassie
        .execute_sql(
            &session,
            "SELECT id FROM declared_id_rows UNION SELECT id FROM declared_id_other",
            vec![],
        )
        .expect("set operation");

    // Assert
    let ordered = vec![vec![Value::Int64(7)], vec![Value::Int64(42)]];
    assert_eq!(derived.rows(), ordered);
    assert_eq!(cte.rows(), ordered);
    let mut union_values = union
        .rows()
        .iter()
        .map(|row| row[0].clone())
        .collect::<Vec<_>>();
    union_values.sort_by_key(|value| match value {
        Value::Int64(number) => *number,
        other => panic!("expected the declared id values, got {other:?}"),
    });
    assert_eq!(
        union_values,
        vec![Value::Int64(7), Value::Int64(42), Value::Int64(99)]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_return_a_value_for_a_column_aliased_to_the_internal_identity_name() {
    // Arrange
    let path = data_dir("reserved_id_aliased_output");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE aliased_identity_rows (name TEXT)",
        "INSERT INTO aliased_identity_rows (name) VALUES ('alice')",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }

    // Act
    let aliased = cassie
        .execute_sql(
            &session,
            "SELECT name AS _id FROM aliased_identity_rows",
            vec![],
        )
        .expect("aliased output column");

    // Assert
    // A row narrower than its own column list desynchronizes the pgwire
    // RowDescription/DataRow pair, so the widths must agree.
    assert_eq!(aliased.columns().len(), 1);
    assert_eq!(
        aliased.rows(),
        vec![vec![Value::String("alice".to_string())]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_drop_a_declared_id_column_but_never_the_internal_identity() {
    // Arrange
    let path = data_dir("reserved_id_drop_column");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE droppable_id_rows (id INT, name TEXT)",
        "INSERT INTO droppable_id_rows (id, name) VALUES (42, 'alice')",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }

    // Act
    let dropped = cassie.execute_sql(
        &session,
        "ALTER TABLE droppable_id_rows DROP COLUMN id",
        vec![],
    );
    let reserved = cassie.execute_sql(
        &session,
        "ALTER TABLE droppable_id_rows DROP COLUMN _id",
        vec![],
    );

    // Assert
    assert!(
        dropped.is_ok(),
        "a declared id column is an ordinary column and must be droppable: {dropped:?}"
    );
    let message = reserved
        .expect_err("dropping _id must be rejected")
        .to_string();
    assert!(
        message.contains("reserved field '_id'"),
        "unexpected error for dropping the internal identity: {message}"
    );

    let _ = std::fs::remove_dir_all(path);
}
