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
        .raw_scan_prefix(StorageFamily::Schema, &[])
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
            cassie
                .midge
                .raw_put(
                    StorageFamily::Schema,
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
