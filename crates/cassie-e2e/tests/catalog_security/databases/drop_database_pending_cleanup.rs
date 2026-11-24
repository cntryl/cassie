//! A refused DROP DATABASE must leave the database's storage metadata intact.

fn cassie_with_deferred_table_drop() -> (
    cassie::app::Cassie,
    cassie::app::CassieSession,
    cassie::runtime::RunningQueryGuard,
) {
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let postgres = cassie.create_session("tester", Some("postgres".to_string()));
    cassie
        .execute_sql(&postgres, "CREATE DATABASE tenant_b", vec![])
        .unwrap();
    let tenant = cassie.create_session("tester", Some("tenant_b".to_string()));
    cassie
        .execute_sql(&tenant, "CREATE TABLE public.docs (title TEXT)", vec![])
        .unwrap();
    cassie
        .execute_sql(&tenant, "INSERT INTO public.docs VALUES ('a')", vec![])
        .unwrap();
    let reader = cassie.begin_schema_epoch_guard_for_diagnostics();
    cassie
        .execute_sql(&tenant, "DROP TABLE public.docs", vec![])
        .unwrap();
    (cassie, postgres, reader)
}

#[test]
fn should_keep_public_schema_in_storage_when_drop_database_is_refused() {
    // Arrange
    let (cassie, postgres, _reader) = cassie_with_deferred_table_drop();

    // Act
    let refused = cassie.execute_sql(&postgres, "DROP DATABASE tenant_b", vec![]);

    // Assert
    assert!(refused.is_err(), "retained rows must refuse the drop");
    assert!(
        cassie
            .midge
            .list_namespaces()
            .iter()
            .any(|namespace| namespace.eq_ignore_ascii_case("tenant_b.public")),
        "refused drop must not remove the public namespace"
    );
}

#[test]
fn should_drop_database_after_deferred_cleanup_drains() {
    // Arrange
    let (cassie, postgres, reader) = cassie_with_deferred_table_drop();
    assert!(cassie
        .execute_sql(&postgres, "DROP DATABASE tenant_b", vec![])
        .is_err());
    drop(reader);
    cassie
        .run_deferred_schema_cleanup_for_diagnostics()
        .unwrap();

    // Act
    let dropped = cassie.execute_sql(&postgres, "DROP DATABASE tenant_b", vec![]);

    // Assert
    assert!(dropped.is_ok(), "drained database must be droppable");
}
