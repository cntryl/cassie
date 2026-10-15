use super::support_read_equivalence::{sql, with_fixture};
use cassie::types::Value;

#[test]
fn should_preserve_zero_limit_in_ordered_top_k() {
    // Arrange
    with_fixture("zero_limit_ordered", |cassie, session| {
        sql(cassie, session, "CREATE TABLE zero_ordered (v INT, w TEXT)");
        sql(
            cassie,
            session,
            "INSERT INTO zero_ordered VALUES (1,'a'),(2,'b'),(3,'c')",
        );
        // Act
        for tail in [
            "ORDER BY v LIMIT 0",
            "ORDER BY v DESC LIMIT 0",
            "ORDER BY w LIMIT 0 OFFSET 1",
            "LIMIT 0",
        ] {
            let result = sql(
                cassie,
                session,
                &format!("SELECT v FROM zero_ordered {tail}"),
            );
            // Assert
            assert!(result.rows().is_empty(), "{tail}: {:?}", result.rows());
        }
    });
}

#[test]
fn should_preserve_typed_row_identity_range_predicates_with_limit() {
    // Arrange
    with_fixture("typed_identity_keyset", |cassie, session| {
        sql(cassie, session, "CREATE TABLE keyset_types (v TEXT)");
        sql(
            cassie,
            session,
            "INSERT INTO keyset_types VALUES ('a'),('b'),('c')",
        );
        // Act
        for (bound, params) in [
            ("0", vec![]),
            ("true", vec![]),
            ("NULL", vec![]),
            ("$1", vec![Value::Int64(0)]),
            ("$1", vec![Value::Bool(true)]),
            ("$1", vec![Value::Null]),
        ] {
            for direction in ["ASC", "DESC"] {
                let statement = format!(
                    "SELECT v FROM keyset_types WHERE _id >= {bound} ORDER BY _id {direction}"
                );
                let baseline = cassie
                    .execute_sql(session, &statement, params.clone())
                    .expect("baseline");
                let bounded = cassie
                    .execute_sql(session, &format!("{statement} LIMIT 10"), params.clone())
                    .expect("bounded");
                // Assert
                assert!(baseline.rows().is_empty(), "{statement}");
                assert_eq!(bounded.rows(), baseline.rows(), "{statement}");
            }
        }
    });
}

#[test]
fn should_preserve_ordered_top_k_on_column_store_tables() {
    // Arrange
    with_fixture("column_ordered_equivalence", |cassie, session| {
        for (table, suffix) in [
            ("ordered_rows", ""),
            ("ordered_columns", " WITH (storage = column_store)"),
        ] {
            sql(
                cassie,
                session,
                &format!("CREATE TABLE {table} (doc_id TEXT, score INT){suffix}"),
            );
            sql(
                cassie,
                session,
                &format!("INSERT INTO {table} VALUES ('d1',1),('d2',2),('d3',3)"),
            );
        }
        // Act
        for tail in [
            "ORDER BY score LIMIT 2",
            "ORDER BY score DESC LIMIT 1",
            "ORDER BY score LIMIT 1 OFFSET 1",
            "ORDER BY doc_id LIMIT 2",
            "ORDER BY score LIMIT 0",
        ] {
            let baseline = sql(
                cassie,
                session,
                &format!("SELECT doc_id FROM ordered_rows {tail}"),
            );
            let column = sql(
                cassie,
                session,
                &format!("SELECT doc_id FROM ordered_columns {tail}"),
            );
            // Assert
            assert_eq!(column.rows(), baseline.rows(), "{tail}");
        }
    });
}
