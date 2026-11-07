//! Transaction-owned COPY descriptor and storage fences.

use super::*;
use crate::app::Cassie;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now())
}

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in ["BEGIN", "CREATE TABLE copy_owner_fence (id INT)"] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    (cassie, session)
}

fn stage_storage_only(cassie: &Cassie, session: &CassieSession) {
    let _scope = cassie
        .midge
        .enter_transactional_ddl_scope(
            session
                .current_database()
                .unwrap_or(&cassie.default_database),
            session.transaction_storage_overlay(),
            true,
        )
        .expect("DDL storage scope");
    let mut sequence = crate::catalog::SequenceMeta::new(
        "copy_owner_storage_only",
        crate::types::DataType::BigInt,
    );
    sequence.object_id = 9001;
    cassie
        .midge
        .put_sequence(&sequence)
        .expect("storage-only change");
}

#[test]
fn should_reject_a_copy_owner_from_a_retired_transaction_generation() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls();
    let owner = cassie
        .capture_copy_command_owner(&session, "postgres.public.copy_owner_fence", &controls)
        .expect("owner");
    session.rollback_transaction();
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("new transaction");

    // Act
    let fork = session.fork_copy_statement_batch(&owner, &controls);

    // Assert
    assert!(fork.is_err());
}

#[test]
fn should_reject_storage_only_changes_before_copy_fork_capture() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls();
    let owner = cassie
        .capture_copy_command_owner(&session, "postgres.public.copy_owner_fence", &controls)
        .expect("owner");
    stage_storage_only(&cassie, &session);

    // Act
    let fork = session.fork_copy_statement_batch(&owner, &controls);

    // Assert
    assert!(fork.is_err());
}

#[test]
fn should_reject_storage_only_changes_after_copy_fork_capture() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls();
    let owner = cassie
        .capture_copy_command_owner(&session, "postgres.public.copy_owner_fence", &controls)
        .expect("owner");
    let fork = session
        .fork_copy_statement_batch(&owner, &controls)
        .expect("fork");
    stage_storage_only(&cassie, &session);

    // Act
    let publication = session.publish_statement_batch(&fork);

    // Assert
    assert!(publication.is_err());
}

#[test]
fn should_reject_a_changed_copy_parent_after_fork_without_rewinding_sequence() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "BEGIN",
        "CREATE TABLE copy_owner_fork (id SERIAL PRIMARY KEY)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let controls = controls();
    let _controls = crate::midge::adapter::PublicationControlScope::enter(Some(&controls));
    let owner = cassie
        .capture_copy_command_owner(&session, "postgres.public.copy_owner_fork", &controls)
        .expect("owner");
    let fork = session
        .fork_copy_statement_batch(&owner, &controls)
        .expect("fork");
    let _fork_scope = cassie
        .midge
        .enter_transactional_ddl_scope(
            session
                .current_database()
                .unwrap_or(&cassie.default_database),
            fork.session().transaction_storage_overlay(),
            false,
        )
        .expect("fork storage scope");
    let first = cassie
        .midge
        .update_sequence_retrying("copy_owner_fork_id_seq", |value| {
            value.advance().map_err(Into::into)
        })
        .expect("fork sequence")
        .0;
    drop(_fork_scope);
    stage_storage_only(&cassie, &session);

    // Act
    let publication = session.publish_statement_batch(&fork);
    let _parent_scope = cassie
        .midge
        .enter_transactional_ddl_scope(
            session
                .current_database()
                .unwrap_or(&cassie.default_database),
            session.transaction_storage_overlay(),
            false,
        )
        .expect("parent storage scope");
    let next = cassie
        .midge
        .update_sequence_retrying("copy_owner_fork_id_seq", |value| {
            value.advance().map_err(Into::into)
        })
        .expect("next sequence")
        .0;

    // Assert
    assert!(publication.is_err());
    assert_eq!((first, next), (1, 2));
    assert!(session.transaction_write_collections().is_empty());
}

#[test]
fn should_use_the_default_database_for_a_session_without_an_explicit_database() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = CassieSession::new("tester".into(), None);
    session.attach_schema_context(&cassie);
    for sql in ["BEGIN", "CREATE TABLE copy_owner_default_db (id INT)"] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let controls = controls();
    let owner = cassie
        .capture_copy_command_owner(&session, "postgres.public.copy_owner_default_db", &controls)
        .expect("owner");

    // Act
    let fork = session.fork_copy_statement_batch(&owner, &controls);

    // Assert
    assert!(fork.is_ok());
}

#[test]
fn should_release_copy_owner_admission_when_the_owner_is_dropped() {
    // Arrange
    let (cassie, session) = fixture();
    let controls = controls();
    let before = controls.current_query_memory_bytes();
    let owner = cassie
        .capture_copy_command_owner(&session, "postgres.public.copy_owner_fence", &controls)
        .expect("owner");
    let retained = controls.current_query_memory_bytes();

    // Act
    drop(owner);

    // Assert
    assert!(retained > before);
    assert_eq!(controls.current_query_memory_bytes(), before);
}

#[test]
fn should_reject_recreated_physical_target_with_an_identical_zero_id_catalog() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE copy_owner_legacy (id INT)", vec![])
        .expect("table");
    let name = "postgres.public.copy_owner_legacy";
    let schema = cassie.midge.collection_schema(name).expect("schema");
    cassie.register_collection(name, schema.clone());
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    let controls = controls();
    let owner = cassie
        .capture_copy_command_owner(&session, name, &controls)
        .expect("physical owner");
    cassie
        .midge
        .drop_collection(name)
        .expect("drop physical target");
    cassie
        .midge
        .create_collection(name, schema.clone())
        .expect("recreate physical target");
    cassie.register_collection(name, schema);

    // Act
    let fork = session.fork_copy_statement_batch(&owner, &controls);

    // Assert
    assert!(fork.is_err());
}

#[test]
fn should_not_pair_old_copy_snapshot_with_recreated_physical_target() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_owner_capture_race (id INT)",
            vec![],
        )
        .expect("table");
    let name = "postgres.public.copy_owner_capture_race";
    let schema = cassie.midge.collection_schema(name).expect("schema");
    cassie.register_collection(name, schema.clone());
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    let controls = controls();
    let before = {
        let overlay = session.transaction_storage_overlay();
        let overlay = overlay.lock();
        cassie
            .midge
            .copy_physical_identity(name, &overlay, &controls)
            .expect("identity")
            .expect("stable id")
    };

    // Act: the callback synchronizes replacement after provisional resolution.
    let owner = cassie
        .capture_bound_copy_command_owner_observed(&session, name, &controls, || {
            cassie.midge.drop_collection(name)?;
            cassie.midge.create_collection(name, schema.clone())?;
            cassie.register_collection(name, schema);
            Ok(())
        })
        .expect("gated recapture");

    // Assert
    let (_, _, physical_id) = owner.0.target.as_ref().expect("bound target");
    assert_ne!(
        *physical_id, before,
        "the owner must pin the replacement observed under the gate"
    );
}

#[test]
fn should_admit_copy_search_path_snapshot_before_allocating() {
    // Arrange
    let session = CassieSession::new("tester".into(), None);
    session.set_search_path(vec!["staged".into(), "public".into()]);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let snapshot = copy_search_path(&session, &controls);
    // Assert
    assert!(snapshot.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_quote_heavy_copy_schema_canonicalization() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 10,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let canonical = copy_canonical_schema("db", "\"\"\"\"", &controls);
    // Assert
    assert!(canonical.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
