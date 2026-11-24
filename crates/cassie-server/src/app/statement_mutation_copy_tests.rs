//! Acknowledged COPY reports remain successful when finishing is unavailable.
use super::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::DocumentWriteOp;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::sql::ast::{CopyFormat, CopyStatement};
use crate::types::Value;

fn fixture() -> (Cassie, crate::app::CassieSession, String) {
    let config = CassieRuntimeConfig::default();
    assert!(config.limits.execution_result_cache_enabled.is_enabled());
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE copy_finish (id BIGINT PRIMARY KEY, k BIGINT)",
        "CREATE TABLE copy_dim (k BIGINT PRIMARY KEY)",
        "INSERT INTO copy_dim VALUES (1)",
        "INSERT INTO copy_finish VALUES (1,1)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let table = crate::catalog::canonical_relation_name(
        &cassie.default_database,
        crate::catalog::DEFAULT_SCHEMA,
        "copy_finish",
    );
    (cassie, session, table)
}

fn count(cassie: &Cassie, session: &crate::app::CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("query")
        .rows()
        .to_vec()
}

#[test]
fn should_publish_acknowledged_copy_epoch_before_unavailable_finishing() {
    // Arrange
    let (cassie, session, table) = fixture();
    cassie.refresh_cardinality_stats(&table).expect("old hint");
    let queries = [
        "SELECT COUNT(*) FROM copy_finish",
        "SELECT COUNT(*) FROM copy_finish f JOIN copy_dim d ON f.k=d.k",
    ];
    for sql in queries {
        assert_eq!(count(&cassie, &session, sql), vec![vec![Value::Int64(1)]]);
        assert_eq!(count(&cassie, &session, sql), vec![vec![Value::Int64(1)]]);
    }
    assert!(
        cassie.metrics()["execution_result_cache"]["hits"]
            .as_u64()
            .expect("cache hits")
            > 0
    );
    let old_epoch = cassie.runtime.data_epoch();
    let mut options = cassie.buffered_document_write_options(&table);
    options.refresh_after_commit = false;
    let report = cassie
        .midge
        .apply_document_write_batch_with_options(
            &table,
            vec![DocumentWriteOp::Put {
                id: "new-copy-row".into(),
                payload: serde_json::json!({"id":2,"k":1}),
            }],
            &options,
        )
        .expect("known acknowledged source commit");
    let cancellation = QueryCancellationHandle::new();
    cancellation.cancel();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
        cancellation,
    );

    // Act
    cassie
        .finish_acknowledged_copy_report(&table, report, Some(&controls))
        .expect("mandatory memory barrier succeeds despite canceled maintenance");

    // Assert
    assert!(cassie.runtime.data_epoch() > old_epoch);
    assert_eq!(
        cassie.runtime.data_epoch(),
        cassie.midge.data_epoch().expect("source epoch")
    );
    assert!(cassie.catalog.get_cardinality_stats(&table).is_none());
    assert!(cassie
        .midge
        .get_cardinality_stats(&table)
        .expect("generation-fenced hint")
        .is_none());
    assert!(
        cassie.metrics()["cardinality"]["unavailable"]
            .as_u64()
            .expect("diagnostic")
            > 0
    );
    assert_eq!(cassie.health()["status"], "ok");
    for sql in queries {
        assert_eq!(count(&cassie, &session, sql), vec![vec![Value::Int64(2)]]);
    }
    assert_eq!(
        count(
            &cassie,
            &session,
            "SELECT id,k FROM copy_finish ORDER BY id"
        ),
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(1)]
        ]
    );
    let generation = cassie
        .midge
        .collection_generation(&table)
        .expect("generation");
    assert!(cassie
        .midge
        .list_maintenance_debt()
        .expect("source debt")
        .iter()
        .any(|debt| {
            debt.collection == table
                && debt.artifact == "projection_hash"
                && debt.target_generation == generation
        }));
}

#[test]
fn should_acknowledge_copy_through_guarded_finishing() {
    // Arrange
    let (cassie, session, _) = fixture();
    let statement = CopyStatement {
        table: "copy_finish".into(),
        columns: vec![],
        format: CopyFormat::Csv,
        header: false,
    };

    // Act
    let affected = cassie
        .copy_from_csv_stdin(&session, &statement, b"2,1\n")
        .expect("acknowledged COPY");

    // Assert
    assert_eq!(affected, 1);
    assert_eq!(
        cassie.runtime.data_epoch(),
        cassie.midge.data_epoch().expect("source epoch")
    );
    assert_eq!(
        count(&cassie, &session, "SELECT COUNT(*) FROM copy_finish"),
        vec![vec![Value::Int64(2)]]
    );
}

fn owner_snapshot(cassie: &Cassie, table: &str) -> serde_json::Value {
    serde_json::json!({
        "hint": cassie.catalog.get_cardinality_stats(table),
        "catalog_version": cassie.catalog.version(),
        "projection": cassie.midge.projection_metadata(table).expect("projection"),
        "hashes": cassie.midge.projection_hash_summary(table).expect("hashes"),
        "debt": cassie.midge.list_maintenance_debt().expect("debt"),
    })
}

#[test]
fn should_preserve_replacement_owner_metadata_when_finishing_a_retired_copy() {
    // Arrange
    let (cassie, session, table) = fixture();
    let mut options = cassie.buffered_document_write_options(&table);
    options.refresh_after_commit = false;
    let report = cassie
        .midge
        .apply_document_write_batch_with_options(
            &table,
            vec![DocumentWriteOp::Put {
                id: "retired-row".into(),
                payload: serde_json::json!({"id":2,"k":1}),
            }],
            &options,
        )
        .expect("old acknowledged source commit");
    let old_identity = report.relation_id;
    for sql in [
        "BEGIN",
        "DROP TABLE copy_finish",
        "CREATE TABLE copy_finish (id BIGINT PRIMARY KEY, k BIGINT)",
        "INSERT INTO copy_finish VALUES (9,1)",
        "COMMIT",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("replacement");
    }
    cassie
        .refresh_cardinality_stats(&table)
        .expect("replacement hint");
    let new_identity = cassie
        .midge
        .row_schema(&table)
        .expect("replacement descriptor")
        .relation_id;
    let before = owner_snapshot(&cassie, &table);

    // Act
    cassie
        .finish_acknowledged_copy_report(&table, report, None)
        .expect("retired finishing");

    // Assert
    assert_ne!(old_identity, new_identity);
    assert_eq!(owner_snapshot(&cassie, &table), before);
    assert_eq!(
        count(&cassie, &session, "SELECT id,k FROM copy_finish"),
        vec![vec![Value::Int64(9), Value::Int64(1)]]
    );
}

#[test]
fn should_flush_the_copy_target_database_family() {
    // Arrange
    let (cassie, admin, _) = fixture();
    cassie
        .execute_sql(&admin, "CREATE DATABASE copy_tenant", vec![])
        .expect("tenant database");
    let tenant = cassie.create_session("tester", Some("copy_tenant".into()));
    cassie
        .execute_sql(&tenant, "CREATE TABLE tenant_copy (id BIGINT)", vec![])
        .expect("tenant table");
    let statement = CopyStatement {
        table: "tenant_copy".into(),
        columns: vec![],
        format: CopyFormat::Csv,
        header: false,
    };
    let expected = cassie
        .midge
        .database_family("copy_tenant")
        .expect("tenant family")
        .id();
    let default = cassie
        .midge
        .database_family(&cassie.default_database)
        .expect("default family")
        .id();
    let _ = crate::midge::adapter::Midge::take_collection_flush_family_for_test();

    // Act
    let affected = cassie
        .copy_from_csv_stdin(&tenant, &statement, b"7\n")
        .expect("tenant COPY");
    let actual = crate::midge::adapter::Midge::take_collection_flush_family_for_test();

    // Assert
    assert_eq!(affected, 1);
    assert_ne!(expected, default);
    assert_eq!(actual, Some(expected));
    assert_eq!(
        count(&cassie, &tenant, "SELECT id FROM tenant_copy"),
        vec![vec![Value::Int64(7)]]
    );
}
