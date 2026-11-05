use cassie::app::{Cassie, CassieError};
use cassie::midge::adapter::StorageFamily;

struct Directory(String);
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("remove closed restored-definition fixture");
    }
}

#[test]
fn should_reject_restored_null_safe_definitions_on_use() {
    // Arrange
    crate::support_sql::use_local_storage();
    let directory = Directory(crate::support_sql::data_dir("null_safe_restored"));
    let cassie = Cassie::new_with_data_dir(&directory.0).expect("create initial engine");
    cassie
        .startup()
        .expect("initialize original database scope");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE VIEW legacy_view AS SELECT TRUE AS same",
        r#"CREATE FUNCTION legacy_function() RETURNS BOOLEAN AS "true""#,
        r#"CREATE PROCEDURE legacy_procedure() AS "SELECT true AS same""#,
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("seed compatible definition");
    }
    let mut modified = 0;
    for (key, raw) in cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &[])
        .expect("scan actual metadata keys")
    {
        let Ok(mut record) = serde_json::from_slice::<serde_json::Value>(&raw) else {
            continue;
        };
        let Some(name) = record.get("name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let replacement = match name.rsplit('.').next().expect("metadata name") {
            "legacy_view" => Some(("query", "SELECT NULL IS NOT DISTINCT FROM NULL AS same")),
            "legacy_function" => Some(("body", "NULL IS NOT DISTINCT FROM NULL")),
            "legacy_procedure" => Some(("body", "SELECT NULL IS NOT DISTINCT FROM NULL AS same")),
            _ => None,
        };
        if let Some((field, sql)) = replacement {
            record[field] = serde_json::Value::String(sql.into());
            record
                .as_object_mut()
                .expect("metadata object")
                .remove("definition_semantics_version");
            cassie
                .midge
                .raw_put(
                    StorageFamily::Data,
                    &key,
                    &serde_json::to_vec(&record).expect("encode actual metadata"),
                )
                .expect("inject unchecked definition");
            modified += 1;
        }
    }
    assert_eq!(modified, 3);
    cassie.shutdown();
    drop(session);
    drop(cassie);
    let restored = Cassie::new_with_data_dir(&directory.0).expect("reopen preserved bytes");
    restored
        .startup()
        .expect("startup retains opaque definition bytes");
    let session = restored.create_session("tester", None);

    // Act
    let results = [
        "SELECT same FROM legacy_view",
        "SELECT legacy_function() AS same",
        "CALL legacy_procedure()",
    ]
    .map(|sql| restored.execute_sql(&session, sql, vec![]));

    // Assert
    assert!(
        results.iter().all(|result| matches!(result,
        Err(CassieError::Unsupported(message)) if message.contains("persisted definitions"))),
        "should_reject_restored_null_safe_definitions_on_use should report Unsupported (SQLSTATE 0A000) for persisted definition"
    );
    restored.shutdown();
}

#[test]
fn should_preserve_versioned_definition_behavior_after_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let directory = Directory(crate::support_sql::data_dir("null_safe_versioned_restart"));
    let cassie = Cassie::new_with_data_dir(&directory.0).expect("local engine");
    cassie.startup().expect("initialize scope");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE persisted_source (id BIGINT,n BIGINT CHECK (n IS DISTINCT FROM NULL),tenant TEXT,event_at TEXT,same BOOLEAN DEFAULT (NULL IS NOT DISTINCT FROM NULL))",
        "INSERT INTO persisted_source(id,n,tenant,event_at) VALUES(1,7,'a','2026-01-01T00:05:00Z')",
        "CREATE VIEW persisted_view AS SELECT id,same FROM persisted_source WHERE n IS NOT DISTINCT FROM 7",
        "CREATE INDEX persisted_predicate ON persisted_source(n) WHERE n IS DISTINCT FROM NULL",
        "CREATE MATERIALIZED PROJECTION persisted_projection AS SELECT n FROM persisted_source WHERE n IS DISTINCT FROM NULL",
        "CREATE ROLLUP persisted_rollup ON persisted_source USING time_bucket('1 hour',event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total WHERE n IS DISTINCT FROM NULL",
    ] { cassie.execute_sql(&session,sql,vec![]).expect("selected persisted definition setup"); }
    cassie.shutdown();
    drop(session);
    drop(cassie);
    let restored = Cassie::new_with_data_dir(&directory.0).expect("restore local engine");
    restored.startup().expect("load selected semantics");
    let session = restored.create_session("tester", None);
    // Act
    let rows = restored
        .execute_sql(&session, "SELECT id,same FROM persisted_view", vec![])
        .expect("restored view")
        .rows()
        .to_vec();
    restored
        .execute_sql(
            &session,
            "ALTER MATERIALIZED PROJECTION persisted_projection BUILD VERSION",
            vec![],
        )
        .expect("restored projection build");
    restored
        .execute_sql(&session, "REFRESH ROLLUP persisted_rollup", vec![])
        .expect("restored rollup refresh");
    let rejected = restored.execute_sql(
        &session,
        "INSERT INTO persisted_source(id,n) VALUES(2,NULL)",
        vec![],
    );
    // Assert
    assert_eq!(
        rows,
        vec![vec![
            cassie::types::Value::Int64(1),
            cassie::types::Value::Bool(true)
        ]]
    );
    assert!(matches!(
        rejected.expect_err("restored CHECK"),
        CassieError::CheckViolation { .. }
    ));
    let view = restored
        .midge
        .get_view("persisted_view")
        .expect("view metadata")
        .expect("view");
    assert_eq!(view.definition_semantics_version, 1);
    let projection = restored
        .catalog
        .get_materialized_projection("persisted_projection")
        .expect("projection");
    assert_eq!(
        projection
            .materialized
            .expect("definition")
            .definition_semantics_version,
        1
    );
    assert_eq!(
        restored
            .catalog
            .get_rollup("persisted_rollup")
            .expect("rollup")
            .definition_semantics_version,
        1
    );
    assert!(restored
        .midge
        .list_indexes()
        .expect("indexes")
        .iter()
        .any(|index| index.name.ends_with("persisted_predicate")
            && index.definition_semantics_version == 1));
    restored.shutdown();
}

#[test]
fn should_preserve_legacy_reader_markers_after_restored_view_use() {
    // Arrange
    crate::support_sql::use_local_storage();
    let directory = Directory(crate::support_sql::data_dir("null_safe_legacy_no_upgrade"));
    let cassie = Cassie::new_with_data_dir(&directory.0).expect("local legacy store");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE legacy_source(n BIGINT)", vec![])
        .expect("legacy table");
    cassie
        .execute_sql(&session, "INSERT INTO legacy_source VALUES(7)", vec![])
        .expect("legacy row");
    let view = cassie::catalog::ViewMeta::new(
        "legacy_plain",
        "SELECT n FROM legacy_source",
        cassie::types::Schema {
            fields: vec![cassie::types::FieldSchema {
                name: "n".into(),
                data_type: cassie::types::DataType::BigInt,
                nullable: true,
            }],
        },
    );
    cassie
        .midge
        .put_view(&view)
        .expect("legacy version-zero publication");
    cassie.shutdown();
    drop(session);
    drop(cassie);
    let restored = Cassie::new_with_data_dir(&directory.0).expect("reopen v1");
    // Act
    restored.startup().expect("restore legacy metadata");
    let session = restored.create_session("tester", None);
    let rows = restored
        .execute_sql(&session, "SELECT n FROM legacy_plain", vec![])
        .expect("legacy view read")
        .rows()
        .to_vec();
    let schema = restored
        .midge
        .raw_scan_prefix(StorageFamily::Schema, &[])
        .expect("reader marker");
    // Assert
    assert_eq!(rows, vec![vec![cassie::types::Value::Int64(7)]]);
    assert!(schema
        .iter()
        .any(|(_, value)| value == b"cassie-midge-layout-v1-0.1.0"));
    assert!(!schema
        .iter()
        .any(|(_, value)| value == b"cassie-midge-layout-v2-0.2.0"));
    restored.shutdown();
}
