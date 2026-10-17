//! Explicit ephemeral storage stays independent of other instances and env mode.

use cassie::app::{Cassie, CassieError, CatalogObjectKind};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

#[test]
fn should_isolate_rows_given_explicit_memory_instances_when_the_same_table_is_created() {
    // Arrange
    crate::support_sql::use_local_storage();
    let first = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("first");
    let second = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("second");
    let first_session = first.create_session("tester", None);
    let second_session = second.create_session("tester", None);
    for (instance, session, value) in [(&first, &first_session, 1), (&second, &second_session, 2)] {
        instance.startup().expect("startup");
        instance
            .execute_sql(
                session,
                "CREATE TABLE memory_rows (id INT4 PRIMARY KEY)",
                vec![],
            )
            .expect("create independent table");
        instance
            .execute_sql(
                session,
                &format!("INSERT INTO memory_rows VALUES ({value})"),
                vec![],
            )
            .expect("insert independent value");
    }

    // Act
    let first_rows = first
        .execute_sql(&first_session, "SELECT id FROM memory_rows", vec![])
        .expect("first rows");
    let second_rows = second
        .execute_sql(&second_session, "SELECT id FROM memory_rows", vec![])
        .expect("second rows");
    drop(first_session);
    drop(first);
    let fresh = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("fresh");
    fresh.startup().expect("fresh startup");
    let fresh_session = fresh.create_session("tester", None);
    let absent = fresh
        .execute_sql(&fresh_session, "SELECT id FROM memory_rows", vec![])
        .expect_err("fresh memory has no prior table");

    // Assert
    assert_eq!(first_rows.rows(), &[vec![Value::Int64(1)]]);
    assert_eq!(second_rows.rows(), &[vec![Value::Int64(2)]]);
    assert!(
        matches!(
            absent,
            CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Relation,
                ..
            }
        ),
        "{absent}"
    );
}
