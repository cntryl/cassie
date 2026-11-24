use super::*;

#[test]
fn should_rollback_failed_add_column_backfill_through_local_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("schema_operation_field_add_atomic_failure");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE field_add_atomic_failure (keep TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO field_add_atomic_failure (keep) VALUES ('alpha'), ('beta')",
            vec![],
        )
        .expect("seed rows");
    let collection = canonical_test_collection(&cassie, "field_add_atomic_failure");

    // Act
    set_document_write_failure_point(Some(DocumentWriteFailurePoint::Row));
    let failed = cassie.execute_sql(
        &session,
        "ALTER TABLE field_add_atomic_failure ADD COLUMN added INT DEFAULT 7",
        vec![],
    );
    let schema_before_restart = cassie
        .midge
        .collection_schema(&collection)
        .expect("read pre-restart schema");
    let rows_before_restart = cassie
        .midge
        .scan_documents(&collection)
        .expect("read pre-restart rows");
    drop(cassie);

    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted
        .startup()
        .expect("start after rejected ADD COLUMN");
    let session = restarted.create_session("tester", None);
    let schema_after_restart = restarted
        .midge
        .collection_schema(&collection)
        .expect("read restarted schema");
    let rows_after_restart = restarted
        .midge
        .scan_documents(&collection)
        .expect("read restarted rows");
    let pending = restarted
        .midge
        .resume_pending_field_add(&collection, "added")
        .expect("check for a pending backfill");
    let visible = restarted
        .execute_sql(
            &session,
            "SELECT keep FROM field_add_atomic_failure ORDER BY keep",
            vec![],
        )
        .expect("read preserved rows");

    // Assert
    assert!(failed.is_err());
    assert!(!schema_before_restart
        .fields
        .iter()
        .any(|field| field.name == "added"));
    assert!(!schema_after_restart
        .fields
        .iter()
        .any(|field| field.name == "added"));
    assert!(rows_before_restart
        .iter()
        .chain(&rows_after_restart)
        .all(|row| !row
            .payload
            .as_object()
            .is_some_and(|payload| payload.contains_key("added"))));
    assert!(
        !pending,
        "failed ADD COLUMN must not leave a pending journal"
    );
    let mut preserved_rows = rows_before_restart
        .iter()
        .map(|row| row.payload["keep"].as_str().expect("original keep value"))
        .collect::<Vec<_>>();
    let mut recovered_rows = rows_after_restart
        .iter()
        .map(|row| row.payload["keep"].as_str().expect("original keep value"))
        .collect::<Vec<_>>();
    preserved_rows.sort_unstable();
    recovered_rows.sort_unstable();
    assert_eq!(preserved_rows, ["alpha", "beta"]);
    assert_eq!(recovered_rows, ["alpha", "beta"]);
    assert_eq!(
        visible.rows(),
        vec![
            vec![cassie::types::Value::String("alpha".into())],
            vec![cassie::types::Value::String("beta".into())]
        ]
    );
    drop(restarted);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resume_only_the_recorded_add_column_definition_on_retry() {
    // Arrange
    use_local_storage();
    let path = data_dir("schema_operation_field_add_retry_definition");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE field_add_retry_definition (keep TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO field_add_retry_definition (keep) VALUES ('alpha')",
            vec![],
        )
        .expect("seed row");

    // Act
    set_field_add_failure_point(true);
    let interrupted = cassie.execute_sql(
        &session,
        "ALTER TABLE field_add_retry_definition ADD COLUMN added INT NOT NULL DEFAULT 7",
        vec![],
    );
    let conflicting_retry = cassie.execute_sql(
        &session,
        "ALTER TABLE field_add_retry_definition ADD COLUMN added BIGINT NOT NULL DEFAULT 8",
        vec![],
    );
    let matching_retry = cassie.execute_sql(
        &session,
            "ALTER TABLE field_add_retry_definition ADD COLUMN IF NOT EXISTS added INT NOT NULL DEFAULT 7",
        vec![],
    );
    let result = cassie.execute_sql(
        &session,
        "SELECT added FROM field_add_retry_definition",
        vec![],
    );

    // Assert
    assert!(interrupted.is_err());
    assert!(conflicting_retry.is_err());
    matching_retry.expect("matching IF NOT EXISTS retry resumes recorded work");
    assert_eq!(
        result.expect("read resumed backfill").rows(),
        vec![vec![cassie::types::Value::Int64(7)]]
    );
    assert!(cassie
        .midge
        .load_constraints(&canonical_test_collection(
            &cassie,
            "field_add_retry_definition"
        ))
        .expect("read resumed constraint metadata")
        .iter()
        .any(|constraint| constraint.field == "added" && constraint.not_null));
    assert!(cassie
        .execute_sql(
            &session,
            "INSERT INTO field_add_retry_definition (keep, added) VALUES ('null', NULL)",
            vec![],
        )
        .is_err());
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_require_recorded_recovery_for_legacy_volatile_default_retry() {
    // Arrange
    use_local_storage();
    let _failpoint_guard = FIELD_ADD_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("schema_operation_field_add_legacy_volatile_retry");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE field_add_legacy_volatile_retry (keep TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO field_add_legacy_volatile_retry (keep) VALUES ('alpha'), ('beta')",
            vec![],
        )
        .expect("seed rows");
    let collection = canonical_test_collection(&cassie, "field_add_legacy_volatile_retry");

    // Act
    set_field_add_failure_point(true);
    let interrupted = cassie.execute_sql(
        &session,
        "ALTER TABLE field_add_legacy_volatile_retry ADD COLUMN made TIMESTAMPTZ DEFAULT now()",
        vec![],
    );
    let durable_schema = cassie
        .midge
        .collection_schema(&collection)
        .expect("read schema after legacy interruption");
    let retry = cassie.execute_sql(
        &session,
        "ALTER TABLE field_add_legacy_volatile_retry ADD COLUMN IF NOT EXISTS made TIMESTAMPTZ DEFAULT now()",
        vec![],
    );
    let rows_after_retry = cassie
        .midge
        .scan_documents(&collection)
        .expect("read rows after rejected legacy retry");
    let schema_after_retry = cassie
        .midge
        .collection_schema(&collection)
        .expect("read schema after rejected legacy retry");
    drop(cassie);

    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted
        .startup()
        .expect("recover the recorded legacy field-add operation");
    let rows = restarted
        .midge
        .scan_documents(&collection)
        .expect("read rows after recorded-state recovery");

    // Assert
    assert!(interrupted.is_err());
    assert!(durable_schema
        .fields
        .iter()
        .any(|field| field.name == "made"));
    assert!(retry.is_err_and(|error| {
        error.sql_state() == "55000"
            && error
                .to_string()
                .contains("durable with a pending backfill")
            && error
                .to_string()
                .contains("restart Cassie to recover the recorded default")
    }));
    assert!(
        schema_after_retry
            .fields
            .iter()
            .any(|field| field.name == "made"),
        "the rejected retry must preserve the durable column"
    );
    assert_eq!(rows_after_retry.len(), 2);
    assert!(rows_after_retry.iter().all(|row| !row
        .payload
        .as_object()
        .is_some_and(|payload| payload.contains_key("made"))));
    assert_eq!(rows.len(), 2);
    let mut values = rows
        .iter()
        .map(|row| {
            row.payload["made"]
                .as_str()
                .expect("recovered volatile default")
        })
        .collect::<Vec<_>>();
    values.sort_unstable();
    assert_eq!(
        values[0], values[1],
        "recovery uses the stored single default"
    );
    drop(restarted);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_require_reapplying_unrecorded_shared_sequence_default_after_legacy_recovery() {
    // Arrange
    use_local_storage();
    let _failpoint_guard = FIELD_ADD_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("schema_operation_field_add_legacy_shared_sequence_retry");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE SEQUENCE legacy_original_seq", vec![])
        .expect("create original shared sequence");
    cassie
        .execute_sql(&session, "CREATE SEQUENCE legacy_retry_seq", vec![])
        .expect("create retry shared sequence");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE field_add_legacy_shared_sequence_retry (keep TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO field_add_legacy_shared_sequence_retry (keep) VALUES ('alpha'), ('beta')",
            vec![],
        )
        .expect("seed rows");
    let collection = canonical_test_collection(&cassie, "field_add_legacy_shared_sequence_retry");

    // Act
    set_field_add_failure_point(true);
    let interrupted = cassie.execute_sql(
        &session,
        "ALTER TABLE field_add_legacy_shared_sequence_retry ADD COLUMN made BIGINT DEFAULT nextval('legacy_original_seq')",
        vec![],
    );
    let retry = cassie.execute_sql(
        &session,
        "ALTER TABLE field_add_legacy_shared_sequence_retry ADD COLUMN IF NOT EXISTS made BIGINT DEFAULT nextval('legacy_retry_seq')",
        vec![],
    );
    drop(cassie);

    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted
        .startup()
        .expect("recover the recorded field-add operation");
    let session = restarted.create_session("tester", None);
    let constraints = restarted
        .midge
        .load_constraints(&collection)
        .expect("read constraints after recorded-state recovery");
    let rows = restarted
        .midge
        .scan_documents(&collection)
        .expect("read rows after recorded-state recovery");
    let reapply = restarted.execute_sql(
        &session,
        "ALTER TABLE field_add_legacy_shared_sequence_retry ALTER COLUMN made SET DEFAULT nextval('legacy_retry_seq')",
        vec![],
    );
    let inserted = restarted.execute_sql(
        &session,
        "INSERT INTO field_add_legacy_shared_sequence_retry (keep) VALUES ('gamma') RETURNING made",
        vec![],
    );

    // Assert
    assert!(interrupted.is_err());
    assert!(retry.is_err_and(|error| {
        error.sql_state() == "55000"
            && error
                .to_string()
                .contains("shared sequence identity cannot be recovered")
            && error
                .to_string()
                .contains("reapply the intended default after recovery")
    }));
    assert_eq!(rows.len(), 2);
    assert!(!constraints
        .iter()
        .any(|constraint| constraint.field == "made" && constraint.default_sequence.is_some()));
    reapply.expect("reapply the intended shared sequence default explicitly");
    assert_eq!(
        inserted
            .expect("insert using the explicitly restored sequence default")
            .rows(),
        vec![vec![cassie::types::Value::Int64(1)]]
    );
    drop(restarted);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_publish_sequence_metadata_after_matching_legacy_add_column_retry() {
    // Arrange
    use_local_storage();
    let path = data_dir("schema_operation_field_add_retry_sequence");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE field_add_retry_sequence (keep TEXT)",
            vec![],
        )
        .expect("create table");
    // Act
    set_field_add_failure_point(true);
    let interrupted = cassie.execute_sql(
        &session,
        "ALTER TABLE field_add_retry_sequence ADD COLUMN added SERIAL",
        vec![],
    );
    let matching_retry = cassie.execute_sql(
        &session,
        "ALTER TABLE field_add_retry_sequence ADD COLUMN IF NOT EXISTS added SERIAL",
        vec![],
    );
    let sequence = cassie
        .midge
        .get_sequence("field_add_retry_sequence_added_seq")
        .expect("read retried sequence");
    let inserted = cassie.execute_sql(
        &session,
        "INSERT INTO field_add_retry_sequence (keep) VALUES ('next') RETURNING added",
        vec![],
    );

    // Assert
    assert!(interrupted.is_err());
    matching_retry.expect("matching retry publishes the generated sequence");
    assert!(
        sequence.is_some(),
        "sequence metadata must be durable after retry"
    );
    assert_eq!(
        inserted
            .expect("insert through resumed serial default")
            .rows(),
        vec![vec![cassie::types::Value::Int64(1)]]
    );
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_replay_add_column_derived_state_after_schema_commit_interruption() {
    // Arrange
    use_local_storage();
    let _failpoint_guard = FIELD_ADD_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("schema_operation_field_add_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE field_add_recovery (keep TEXT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO field_add_recovery (keep) VALUES ('alpha'), ('beta')",
            vec![],
        )
        .expect("seed rows");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX field_add_recovery_cover ON field_add_recovery USING column (keep)",
            vec![],
        )
        .expect("create column index");
    let collection = canonical_test_collection(&cassie, "field_add_recovery");
    // Act
    set_field_add_failure_point(true);
    let session = cassie.create_session("tester", None);
    assert!(cassie
        .execute_sql(
            &session,
            "ALTER TABLE field_add_recovery ADD COLUMN added INT DEFAULT 7",
            vec![],
        )
        .is_err());
    let schema_after_interrupt = cassie
        .midge
        .collection_schema(&collection)
        .expect("schema remains durable");
    assert!(schema_after_interrupt
        .fields
        .iter()
        .any(|field| field.name == "added"));
    write_added_field_before_recovery(&cassie.midge, &collection);
    drop(cassie);

    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("replay field add");
    let restarted_session = restarted.create_session("tester", None);
    let result = restarted
        .execute_sql(
            &restarted_session,
            "SELECT keep, added FROM field_add_recovery ORDER BY keep",
            vec![],
        )
        .expect("query after field-add replay");
    let root = restarted
        .midge
        .root_hash(&collection)
        .expect("read rebuilt projection root")
        .expect("projection root after replay");
    let generation = restarted
        .midge
        .collection_generation(&collection)
        .expect("read collection generation");
    let metadata = restarted
        .midge
        .get_column_batch_metadata(&collection, "field_add_recovery_cover")
        .expect("read rebuilt column batches")
        .expect("column batches after replay");
    drop(restarted);
    let restarted_again = Cassie::new_with_data_dir(&path).expect("reopen Cassie again");
    restarted_again
        .startup()
        .expect("replay field add idempotently");

    // Assert
    assert_eq!(
        result.rows(),
        vec![
            vec![
                cassie::types::Value::String("alpha".to_string()),
                cassie::types::Value::Int64(42),
            ],
            vec![
                cassie::types::Value::String("beta".to_string()),
                cassie::types::Value::Int64(7),
            ],
        ]
    );
    assert_eq!(generation, 3);
    assert_eq!(root.built_generation, generation);
    assert_eq!(metadata.built_generation, generation);
    assert!(restarted_again
        .midge
        .root_hash(&collection)
        .expect("read root after second restart")
        .is_some());

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_resume_add_column_default_backfill_after_batch_interruption() {
    // Arrange
    use_local_storage();
    let _failpoint_guard = FIELD_ADD_BACKFILL_FAILPOINT_GUARD.lock().unwrap();
    let path = data_dir("schema_operation_field_add_backfill_recovery");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE field_add_backfill_recovery (keep TEXT)",
            vec![],
        )
        .expect("create table");
    let collection = canonical_test_collection(&cassie, "field_add_backfill_recovery");
    let documents = (0..257)
        .map(|index| (None, serde_json::json!({"keep": format!("row-{index}")})))
        .collect();
    cassie
        .midge
        .put_documents(&collection, documents)
        .expect("seed more rows than one backfill batch");

    // Act
    set_field_add_backfill_failure_point(true);
    assert!(cassie
        .execute_sql(
            &session,
            "ALTER TABLE field_add_backfill_recovery ADD COLUMN added INT DEFAULT 7",
            vec![],
        )
        .is_err());
    let interrupted_rows = cassie
        .midge
        .scan_documents(&collection)
        .expect("read rows after the interrupted backfill");
    let backfilled_count = interrupted_rows
        .iter()
        .filter(|row| row.payload.get("added").and_then(serde_json::Value::as_i64) == Some(7))
        .count();
    drop(cassie);

    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("resume field-add backfill");
    let recovered_rows = restarted
        .midge
        .scan_documents(&collection)
        .expect("read rows after backfill recovery");
    drop(restarted);

    // Assert
    assert_eq!(backfilled_count, 256);
    assert_eq!(recovered_rows.len(), 257);
    assert!(recovered_rows
        .iter()
        .all(|row| { row.payload.get("added").and_then(serde_json::Value::as_i64) == Some(7) }));

    let _ = std::fs::remove_dir_all(path);
}
