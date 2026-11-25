//! A command overlay may advance while its committed native view stays fixed.

use std::sync::Arc;
use std::time::Instant;

use crate::app::{Cassie, CassieError, CassieSession, SessionReadScope};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::midge::adapter::{Midge, StatementDataRead, StatementReadScope};
use crate::runtime::QueryExecutionControls;

#[path = "tests/replay_cloud_cache_loss.rs"]
mod replay_cloud_cache_loss;
#[path = "tests/replay_durable_snapshot.rs"]
mod replay_durable_snapshot;
#[path = "tests/replay_process_crash.rs"]
mod replay_process_crash;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE command_overlay (value INT)",
            Vec::new(),
        )
        .expect("table");
    cassie
        .midge
        .put_document(
            "command_overlay",
            Some("base".to_owned()),
            serde_json::json!({"value": 1}),
        )
        .expect("seed");
    session.begin_transaction(None).expect("begin");
    (cassie, session)
}

fn payload(
    cassie: &Cassie,
    session: &CassieSession,
    owner: &Arc<StatementDataRead>,
    id: &str,
) -> Option<serde_json::Value> {
    let _data = StatementReadScope::enter(Some(owner));
    let _overlay = SessionReadScope::enter(owner.overlay());
    cassie
        .get_document_for_session(Some(session), "command_overlay", id)
        .expect("read")
        .map(|document| document.payload)
}

#[test]
fn should_keep_committed_data_frozen_across_command_overlays() {
    // Arrange
    let (cassie, session) = fixture();
    let original = controls(16 * 1024 * 1024);
    let retained = StatementDataRead::capture_repeatable_read_with_overlay(
        &cassie.midge,
        "postgres",
        &original,
        None,
    )
    .expect("retained Data");
    session
        .stage_document_write(
            "command_overlay",
            "own".to_owned(),
            serde_json::json!({"value": 2}),
        )
        .expect("first own change");
    let first_controls = controls(16 * 1024 * 1024);
    let first_overlay = session
        .capture_statement_overlay(&first_controls)
        .expect("first overlay");
    let first = StatementDataRead::with_overlay(&retained, Some(first_overlay), &first_controls)
        .expect("first command");
    session.create_savepoint("before_later").expect("savepoint");
    session
        .stage_document_write(
            "command_overlay",
            "later".to_owned(),
            serde_json::json!({"value": 3}),
        )
        .expect("later own change");
    cassie
        .midge
        .put_document(
            "command_overlay",
            Some("base".to_owned()),
            serde_json::json!({"value": 9}),
        )
        .expect("other committed change");
    let second_controls = controls(16 * 1024 * 1024);
    let second_overlay = session
        .capture_statement_overlay(&second_controls)
        .expect("second overlay");
    let second = StatementDataRead::with_overlay(&retained, Some(second_overlay), &second_controls)
        .expect("second command");

    // Act
    session
        .rollback_to_savepoint("before_later")
        .expect("restore own changes");
    let third_controls = controls(16 * 1024 * 1024);
    let third_overlay = session
        .capture_statement_overlay(&third_controls)
        .expect("restored overlay");
    let third = StatementDataRead::with_overlay(&retained, Some(third_overlay), &third_controls)
        .expect("restored command");

    // Assert
    assert!(first.shares_native_snapshot_for_test(&second));
    assert!(second.shares_native_snapshot_for_test(&third));
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(
        first.overlay().expect("first command overlay"),
        second.overlay().expect("second command overlay")
    ));
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(
        first.overlay().expect("first command overlay"),
        second.overlay().expect("second command overlay")
    ));
    for owner in [&first, &second, &third] {
        assert_eq!(
            payload(&cassie, &session, owner, "base"),
            Some(serde_json::json!({"value": 1}))
        );
        assert_eq!(
            payload(&cassie, &session, owner, "own"),
            Some(serde_json::json!({"value": 2}))
        );
    }
    assert_eq!(payload(&cassie, &session, &first, "later"), None);
    assert_eq!(
        payload(&cassie, &session, &second, "later"),
        Some(serde_json::json!({"value": 3}))
    );
    assert_eq!(payload(&cassie, &session, &third, "later"), None);
}

#[test]
fn should_release_each_command_admission_after_its_last_owner() {
    // Arrange
    let (cassie, session) = fixture();
    let original = controls(16 * 1024 * 1024);
    let retained = StatementDataRead::capture_repeatable_read_with_overlay(
        &cassie.midge,
        "postgres",
        &original,
        None,
    )
    .expect("retained Data");
    let initial_charge = original.current_query_memory_bytes();
    let command_controls = controls(16 * 1024 * 1024);
    let overlay = session
        .capture_statement_overlay(&command_controls)
        .expect("overlay");
    let command = StatementDataRead::with_overlay(&retained, Some(overlay), &command_controls)
        .expect("command");
    let alias = Arc::clone(&command);
    let command_charge = command_controls.current_query_memory_bytes();

    // Act
    drop(retained);
    let shared_charge = original.current_query_memory_bytes();
    drop(command);
    let aliased_charge = command_controls.current_query_memory_bytes();
    drop(alias);

    // Assert
    assert!(initial_charge > shared_charge);
    assert!(shared_charge > 0);
    assert!(command_charge > 0);
    assert_eq!(aliased_charge, command_charge);
    assert_eq!(original.current_query_memory_bytes(), 0);
    assert_eq!(command_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_command_ownership_without_admitting_its_retained_core_again() {
    // Arrange
    let (cassie, _) = fixture();
    let original = controls(16 * 1024 * 1024);
    let retained = StatementDataRead::capture_repeatable_read_with_overlay(
        &cassie.midge,
        "postgres",
        &original,
        None,
    )
    .expect("retained Data");
    let charged = original.current_query_memory_bytes();
    let denied = controls(1);

    // Act
    let command = StatementDataRead::with_overlay(&retained, None, &denied);

    // Assert
    assert!(matches!(command, Err(CassieError::ResourceLimit(_))));
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(original.current_query_memory_bytes(), charged);
}

#[test]
fn should_rebind_a_command_view_to_its_atomic_working_session() {
    // Arrange
    let (cassie, session) = fixture();
    let original = controls(16 * 1024 * 1024);
    let retained = StatementDataRead::capture_repeatable_read_with_overlay(
        &cassie.midge,
        "postgres",
        &original,
        None,
    )
    .expect("retained Data");
    session
        .stage_document_write(
            "command_overlay",
            "own".to_owned(),
            serde_json::json!({"value": 2}),
        )
        .expect("precommand own row");
    let overlay = session
        .capture_statement_overlay(&original)
        .expect("command overlay");
    let command =
        StatementDataRead::with_overlay(&retained, Some(overlay), &original).expect("command");
    session
        .stage_document_write(
            "command_overlay",
            "later".to_owned(),
            serde_json::json!({"value": 3}),
        )
        .expect("earlier CTE effect");
    let batch = session
        .fork_statement_batch()
        .expect("later CTE working fork");
    let working = batch.session();
    let rebound_controls = controls(16 * 1024 * 1024);

    // Act
    let rebound = working
        .rebind_statement_read(&command, &rebound_controls)
        .expect("rebound command");
    let original_matches_working = command
        .overlay()
        .expect("source overlay")
        .matches_session(working);
    let rebound_matches_working = rebound
        .overlay()
        .expect("rebound overlay")
        .matches_session(working);
    let rebound_matches_original = rebound
        .overlay()
        .expect("rebound overlay")
        .matches_session(&session);
    let later = payload(&cassie, working, &rebound, "later");
    let own = payload(&cassie, working, &rebound, "own");
    drop(rebound);

    // Assert
    assert!(!original_matches_working);
    assert!(rebound_matches_working);
    assert!(!rebound_matches_original);
    assert_eq!(later, None);
    assert_eq!(own, Some(serde_json::json!({"value": 2})));
    assert_eq!(rebound_controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_descriptor_authority_when_command_owners_share_a_core() {
    // Arrange
    for repeatable in [false, true] {
        let (cassie, session) = fixture();
        session.rollback_transaction();
        let original = controls(16 * 1024 * 1024);
        let retained = if repeatable {
            StatementDataRead::capture_repeatable_read_with_overlay(
                &cassie.midge,
                "postgres",
                &original,
                None,
            )
        } else {
            StatementDataRead::capture_with_overlay(&cassie.midge, "postgres", &original, None)
        }
        .expect("selected committed core");
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE command_overlay ADD COLUMN note TEXT",
                Vec::new(),
            )
            .expect("later encoding descriptor");
        let command_controls = controls(16 * 1024 * 1024);

        // Act
        let command = StatementDataRead::with_overlay(&retained, None, &command_controls)
            .expect("command owner");
        let _scope = StatementReadScope::enter(Some(&command));
        let schema = cassie
            .midge
            .row_schema("command_overlay")
            .expect("selected descriptor");

        // Assert
        assert_eq!(
            schema.fields.iter().any(|field| field.name == "note"),
            repeatable
        );
    }
}

fn replay_snapshot_batch(
    projection: &str,
    position: u64,
    value: i64,
) -> crate::app::ProjectionReplayBatch {
    crate::app::ProjectionReplayBatch {
        projection: projection.to_owned(),
        source_identity: "stream".to_owned(),
        batch_id: format!("batch-{position}"),
        lag: 0,
        events: vec![crate::app::ProjectionReplayEvent {
            event_id: format!("event-{position}"),
            checkpoint: format!("checkpoint-{position}"),
            position: Some(position),
            document_id: "doc".to_owned(),
            payload: Some(serde_json::json!({"value": value})),
        }],
    }
}

fn replay_snapshot_has_event(
    midge: &Midge,
    owner: &Arc<StatementDataRead>,
    projection: &str,
    event: &str,
) -> bool {
    let _scope = StatementReadScope::enter(Some(owner));
    midge
        .has_projection_event(projection, "stream", event)
        .expect("read replay event")
}

fn replay_snapshot_index_ids(
    cassie: &Cassie,
    index: &crate::catalog::IndexMeta,
    controls: &QueryExecutionControls,
    owner: &Arc<StatementDataRead>,
    value: i64,
) -> Vec<String> {
    let _scope = StatementReadScope::enter(Some(owner));
    let request = crate::midge::adapter::ScalarIndexScanRequest {
        equality_prefix: vec![serde_json::json!(value)],
        ..Default::default()
    };
    cassie
        .midge
        .scan_scalar_index_controlled(index, &request, &controls.storage_request())
        .unwrap()
        .as_slice()
        .iter()
        .map(|hit| hit.id.clone())
        .collect()
}

#[test]
fn should_preserve_atomic_replay_in_pinned_data_snapshot() {
    // Arrange

    let cassie =
        Arc::new(Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap());
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE replay_storage_snapshot (value INT)",
            vec![],
        )
        .unwrap();
    let projection = cassie
        .midge
        .canonical_collection_name("replay_storage_snapshot");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX replay_storage_value_idx ON replay_storage_snapshot USING btree (value)",
            vec![],
        )
        .unwrap();
    let index = cassie
        .midge
        .get_index(&projection, "replay_storage_value_idx")
        .unwrap()
        .unwrap();
    cassie
        .replay_projection_batch(replay_snapshot_batch(&projection, 1, 1))
        .unwrap();
    let next = replay_snapshot_batch(&projection, 2, 2);
    let query_controls = controls(16 * 1024 * 1024);
    let retained = StatementDataRead::capture(&cassie.midge, "postgres", &query_controls).unwrap();
    let writer = Arc::clone(&cassie);

    // Act
    std::thread::spawn(move || writer.replay_projection_batch(next))
        .join()
        .unwrap()
        .unwrap();
    let old_checkpoint = retained
        .projection_metadata_for_test(&projection)
        .unwrap()
        .unwrap()
        .source_checkpoint;
    let old_row = {
        let _scope = StatementReadScope::enter(Some(&retained));
        cassie
            .midge
            .get_document(&projection, "doc")
            .unwrap()
            .unwrap()
            .payload
    };
    let fresh = StatementDataRead::capture(&cassie.midge, "postgres", &query_controls).unwrap();
    let old_event_1 = replay_snapshot_has_event(&cassie.midge, &retained, &projection, "event-1");
    let old_event_2 = replay_snapshot_has_event(&cassie.midge, &retained, &projection, "event-2");
    let new_event_2 = replay_snapshot_has_event(&cassie.midge, &fresh, &projection, "event-2");
    let old_index_1 = replay_snapshot_index_ids(&cassie, &index, &query_controls, &retained, 1);
    let old_index_2 = replay_snapshot_index_ids(&cassie, &index, &query_controls, &retained, 2);
    let new_index_1 = replay_snapshot_index_ids(&cassie, &index, &query_controls, &fresh, 1);
    let new_index_2 = replay_snapshot_index_ids(&cassie, &index, &query_controls, &fresh, 2);
    let new_checkpoint = fresh
        .projection_metadata_for_test(&projection)
        .unwrap()
        .unwrap()
        .source_checkpoint;
    let new_row = {
        let _scope = StatementReadScope::enter(Some(&fresh));
        cassie
            .midge
            .get_document(&projection, "doc")
            .unwrap()
            .unwrap()
            .payload
    };

    // Assert
    assert_eq!(old_checkpoint.as_deref(), Some("checkpoint-1"));
    assert_eq!(old_row, serde_json::json!({"value": 1}));
    assert!(old_event_1);
    assert!(!old_event_2);
    assert!(new_event_2);
    assert_eq!(old_index_1, vec!["doc".to_owned()]);
    assert_eq!(old_index_2, Vec::<String>::new());
    assert_eq!(new_index_1, Vec::<String>::new());
    assert_eq!(new_index_2, vec!["doc".to_owned()]);
    assert_eq!(new_checkpoint.as_deref(), Some("checkpoint-2"));
    assert_eq!(new_row, serde_json::json!({"value": 2}));
}

#[test]
fn should_charge_private_materialized_rows_to_bound_command_retention() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    let payload = "x".repeat(8192);
    for sql in [
        "CREATE TABLE bind_mv_src (k INT, label TEXT)".to_string(),
        format!("INSERT INTO bind_mv_src VALUES (1, '{payload}')"),
        "CREATE MATERIALIZED VIEW bind_mv AS SELECT k,label FROM bind_mv_src".into(),
        "CREATE UNIQUE INDEX bind_mv_key ON bind_mv(k)".into(),
        "BEGIN ISOLATION LEVEL REPEATABLE READ".into(),
        "SELECT label FROM bind_mv WHERE k=1".into(),
        "REFRESH MATERIALIZED VIEW CONCURRENTLY bind_mv".into(),
    ] {
        cassie.execute_sql(&session, &sql, vec![]).unwrap();
    }
    let command = controls(16 * 1024 * 1024);

    // Act
    let owner = session
        .capture_statement_read(&cassie.midge, "postgres", &command)
        .unwrap();
    let retained = owner.command_accounted_bytes().unwrap();

    // Assert
    assert!(
        retained >= payload.len(),
        "private row bytes in bound command: {retained}"
    );
}

#[test]
fn should_read_an_unrelated_table_with_a_small_budget_after_private_refreshes() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    let payload = "x".repeat(160 * 1024);
    for sql in [
        "CREATE TABLE unrelated_mv_src (k INT, label TEXT)".to_owned(),
        format!("INSERT INTO unrelated_mv_src VALUES (1, '{payload}')"),
        "CREATE TABLE unrelated_small (n INT)".into(),
        "INSERT INTO unrelated_small VALUES (7)".into(),
        "CREATE MATERIALIZED VIEW private_first AS SELECT k,label FROM unrelated_mv_src".into(),
        "CREATE MATERIALIZED VIEW private_second AS SELECT k,label FROM unrelated_mv_src".into(),
        "CREATE UNIQUE INDEX private_first_idx ON private_first(k)".into(),
        "CREATE UNIQUE INDEX private_second_idx ON private_second(k)".into(),
        "BEGIN ISOLATION LEVEL REPEATABLE READ".into(),
        "REFRESH MATERIALIZED VIEW CONCURRENTLY private_first".into(),
        "REFRESH MATERIALIZED VIEW CONCURRENTLY private_second".into(),
    ] {
        cassie.execute_sql(&session, &sql, vec![]).unwrap();
    }
    // Admit the two catalog views invalidated by DDL before measuring Data reads.
    // Neither capture opens a Data reader or sees the private output's row bytes.
    let catalog_controls = controls(16 * 1024 * 1024);
    let catalog = session
        .command_catalog_snapshot(&catalog_controls)
        .unwrap()
        .unwrap();
    let _descriptors = session
        .command_descriptor_snapshot(&cassie.publication_catalog, &catalog_controls)
        .unwrap()
        .unwrap();
    let data_budget = 128 * 1024;
    let small = controls(data_budget);

    // Act
    let result = cassie.execute_sql_with_controls(
        &session,
        "SELECT n FROM unrelated_small",
        vec![],
        crate::runtime::ExecutionMode::SimpleQuery,
        &small,
    );

    let bind = |sql: &str| {
        let retention = controls(catalog.accounted_bytes() + data_budget);
        let parsed = crate::sql::parse_statement(sql).unwrap();
        let fingerprint = crate::runtime::sql_fingerprint(&parsed);
        cassie.validate_pgwire_bind(
            &session,
            parsed,
            fingerprint,
            &[],
            &[],
            (&crate::runtime::QueryCancellationHandle::new(), &retention),
        )
    };
    let unrelated = bind("SELECT n FROM unrelated_small");
    let relevant = bind("SELECT label FROM private_first WHERE k=1");

    // Assert
    assert_eq!(
        result.unwrap().rows(),
        vec![vec![cassie_core::types::Value::Int64(7)]]
    );
    assert!(unrelated.expect("unrelated portal fits").is_some());
    assert!(matches!(relevant, Err(CassieError::ResourceLimit(_))));
}

#[test]
fn should_restore_session_private_outputs_when_rolling_back_a_savepoint() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE private_output_source (id INT PRIMARY KEY)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            &session,
            "INSERT INTO private_output_source VALUES (1)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            &session,
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(&session, "SAVEPOINT before_private_output", vec![])
        .unwrap();

    // Act
    cassie
        .execute_sql(
            &session,
            "CREATE MATERIALIZED VIEW private_output_saved AS \
             SELECT id FROM private_output_source",
            vec![],
        )
        .unwrap();
    let retained = session.transaction_private_outputs();
    session
        .rollback_to_savepoint("before_private_output")
        .unwrap();
    let restored = session.transaction_private_outputs();

    // Assert
    assert!(retained.is_some());
    assert!(restored.is_none());
}
