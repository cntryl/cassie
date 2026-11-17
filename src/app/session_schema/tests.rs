//! Read aliases retain their leases while runtime publishers use mutable metadata.

use super::*;
use crate::app::Cassie;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryExecutionControls;
use crate::types::Value;
use std::sync::Arc;
use std::time::Instant;

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE scoped_identity (id SERIAL PRIMARY KEY, payload TEXT UNIQUE)",
            Vec::new(),
        )
        .expect("existing identity");
    cassie
        .execute_sql(
            &session,
            "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
            Vec::new(),
        )
        .expect("begin");
    cassie
        .execute_sql(&session, "CREATE TABLE scoped_own (id INT)", Vec::new())
        .expect("own DDL");
    (cassie, session)
}

#[test]
fn should_keep_a_late_statement_fork_schema_attachment_out_of_the_replacement() {
    // Arrange
    let (cassie, session) = fixture();
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = session
        .command_descriptor_snapshot(&cassie.publication_catalog, &controls)
        .unwrap()
        .unwrap();
    let mut scoped = cassie.clone();
    scoped.catalog = owner.catalog().clone();
    scoped.catalog_owner = Some(Arc::clone(&owner));
    scoped.metadata_write_catalog = session.transaction_catalog_overlay().unwrap();
    session.attach_schema_context(&scoped).unwrap();
    let batch = cassie.new_statement_batch(Some(&session)).unwrap();
    session.rollback_transaction();
    session.begin_transaction(None).unwrap();
    session.attach_schema_context(&cassie).unwrap();

    // Act
    batch.session().attach_schema_context(&scoped).unwrap();

    // Assert
    assert!(!Arc::ptr_eq(
        &session.schema_context,
        &batch.session().schema_context
    ));
    assert!(session
        .schema_context
        .lock()
        .as_ref()
        .unwrap()
        .owner
        .is_none());
    assert!(Arc::ptr_eq(
        batch
            .session()
            .schema_context
            .lock()
            .as_ref()
            .unwrap()
            .owner
            .as_ref()
            .unwrap(),
        &owner
    ));
    assert!(!session
        .schema_context
        .lock()
        .as_ref()
        .unwrap()
        .catalog
        .relation_exists("scoped_own"));
    drop(batch);
    drop(scoped);
    drop(owner);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_fork_context_payload_and_database_bytes_until_its_last_clone_retires() {
    // Arrange
    let (cassie, session) = fixture();
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let expected = std::mem::size_of::<OwnedSessionSchemaContext>()
        + 2 * std::mem::size_of::<usize>()
        + cassie.default_database.len();

    // Act
    let context = session
        .schema_context
        .fork_with_controls(Some(&controls))
        .unwrap();
    let last = Arc::clone(&context);
    let charged = controls.current_query_memory_bytes();
    drop(context);
    let retained = controls.current_query_memory_bytes();
    drop(last);
    let released = controls.current_query_memory_bytes();
    let exact = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: expected,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let short = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: expected - 1,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let accepted = session.schema_context.fork_with_controls(Some(&exact));
    let denied = session.schema_context.fork_with_controls(Some(&short));

    // Assert
    assert_eq!(charged, expected);
    assert_eq!(retained, expected);
    assert_eq!(released, 0);
    assert!(accepted.is_ok());
    assert_eq!(denied.unwrap_err().sql_state(), "54000");
}

#[test]
fn should_retire_the_schema_context_read_owner_at_transaction_end() {
    // Arrange
    for commit in [false, true] {
        let (cassie, session) = fixture();
        let controls =
            QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
        let owner = session
            .command_descriptor_snapshot(&cassie.publication_catalog, &controls)
            .expect("descriptors")
            .expect("owner");
        let bytes = owner.accounted_bytes();
        let mut scoped = cassie.clone();
        scoped.catalog = owner.catalog().clone();
        scoped.catalog_owner = Some(Arc::clone(&owner));
        scoped.metadata_write_catalog = session
            .transaction_catalog_overlay()
            .expect("mutable stage");
        session.attach_schema_context(&scoped).unwrap();
        drop(scoped);

        // Act
        if commit {
            session.commit_transaction();
        } else {
            session.rollback_transaction();
        }
        let caller_charge = controls.current_query_memory_bytes();
        drop(owner);

        // Assert
        assert_eq!(caller_charge, bytes);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        assert!(session
            .schema_context
            .lock()
            .as_ref()
            .expect("canonical context")
            .owner
            .is_none());
    }
}

#[test]
fn should_preserve_descriptor_metadata_during_sequence_mutations() {
    // Arrange
    let (cassie, session) = fixture();
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = session
        .command_descriptor_snapshot(&cassie.publication_catalog, &controls)
        .expect("descriptors")
        .expect("owner");
    let sequence = owner
        .catalog()
        .list_sequences()
        .into_iter()
        .find(|sequence| {
            sequence
                .owned_by
                .as_ref()
                .is_some_and(|owner| crate::catalog::name_matches(&owner.table, "scoped_identity"))
        })
        .expect("owned sequence");
    let before = sequence.cursor();

    // Act
    let inserted = cassie
        .execute_sql(
            &session,
            "INSERT INTO scoped_identity (payload) VALUES ('one') RETURNING id",
            Vec::new(),
        )
        .expect("working defaults");
    let advanced = cassie
        .execute_sql(
            &session,
            &format!("SELECT nextval('{}')", sequence.name),
            Vec::new(),
        )
        .expect("session nextval");
    cassie
        .execute_sql(
            &session,
            &format!("SELECT setval('{}', 40, false)", sequence.name),
            Vec::new(),
        )
        .expect("session setval");
    cassie
        .execute_sql(
            &session,
            "TRUNCATE scoped_identity RESTART IDENTITY",
            Vec::new(),
        )
        .expect("restart");
    session.rollback_transaction();
    let restored = cassie
        .execute_sql(
            &session,
            &format!("SELECT nextval('{}')", sequence.name),
            Vec::new(),
        )
        .expect("restart rollback");
    let retained = owner
        .catalog()
        .get_sequence(&sequence.name)
        .expect("retained definition")
        .cursor();
    drop(owner);

    // Assert
    assert_eq!(inserted.rows(), &[vec![Value::Int64(1)]]);
    assert_eq!(advanced.rows(), &[vec![Value::Int64(2)]]);
    assert_eq!(restored.rows(), &[vec![Value::Int64(40)]]);
    assert_eq!(retained, before);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_keep_published_sequence_cursor_out_of_the_transaction_catalog_overlay() {
    // Arrange
    let (cassie, session) = fixture();
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let owner = session
        .command_descriptor_snapshot(&cassie.publication_catalog, &controls)
        .expect("initial descriptors")
        .expect("repeatable-read descriptor owner");
    let sequence = owner
        .catalog()
        .list_sequences()
        .into_iter()
        .find(|sequence| {
            sequence
                .owned_by
                .as_ref()
                .is_some_and(|owner| crate::catalog::name_matches(&owner.table, "scoped_identity"))
        })
        .expect("published sequence");
    let before = sequence.cursor();
    let mut updated = sequence.clone();
    updated.current_value = 40;
    updated.is_called = true;
    let mut scoped = cassie.clone();
    scoped.catalog = owner.catalog().clone();
    scoped.catalog_owner = Some(Arc::clone(&owner));
    scoped.metadata_write_catalog = session
        .transaction_catalog_overlay()
        .expect("transaction catalog overlay");
    session.attach_schema_context(&scoped).unwrap();

    // Act
    session.publish_sequence_metadata(&updated);
    let staged = session
        .transaction_catalog_overlay()
        .expect("transaction catalog overlay remains")
        .get_sequence(&sequence.name)
        .expect("staged sequence remains present");
    let refreshed = session
        .command_descriptor_snapshot(&cassie.publication_catalog, &controls)
        .expect("refresh descriptors after cursor publication")
        .expect("refreshed owner");
    let visible = refreshed
        .catalog()
        .get_sequence(&sequence.name)
        .expect("sequence remains visible");
    session.rollback_transaction();

    // Assert
    assert_eq!(staged.cursor(), before);
    assert_eq!(visible.current_value, 40);
    assert!(visible.is_called);
    assert!(!Arc::ptr_eq(&owner, &refreshed));
}

#[test]
fn should_preserve_once_only_upsert_defaults_before_the_working_fork() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let inserted = cassie.execute_sql(&session, "INSERT INTO scoped_identity (payload) VALUES ('one') ON CONFLICT (payload) DO NOTHING RETURNING id", Vec::new()).expect("first prepared candidate");
    let skipped = cassie.execute_sql(&session, "INSERT INTO scoped_identity (payload) VALUES ('one') ON CONFLICT (payload) DO NOTHING RETURNING id", Vec::new()).expect("skipped candidate still consumes default once");
    let following = cassie
        .execute_sql(
            &session,
            "INSERT INTO scoped_identity (payload) VALUES ('two') RETURNING id",
            Vec::new(),
        )
        .expect("following default");
    session.rollback_transaction();

    // Assert
    assert_eq!(inserted.rows(), &[vec![Value::Int64(1)]]);
    assert_eq!(skipped.rows(), &[] as &[Vec<Value>]);
    assert_eq!(following.rows(), &[vec![Value::Int64(3)]]);
}

#[test]
fn should_preserve_prepared_defaults_across_an_owner_wait() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let owner = cassie.create_session("owner", None);
    for sql in [
        "CREATE TABLE waiting_identity (id SERIAL PRIMARY KEY, payload TEXT UNIQUE)",
        "BEGIN",
        "INSERT INTO waiting_identity (payload) VALUES ('held')",
    ] {
        cassie
            .execute_sql(&owner, sql, Vec::new())
            .expect("held unique key");
    }
    let waiter = cassie.create_session("waiter", None);
    for sql in [
        "BEGIN ISOLATION LEVEL REPEATABLE READ READ WRITE",
        "CREATE TABLE waiting_own (id INT)",
    ] {
        cassie
            .execute_sql(&waiter, sql, Vec::new())
            .expect("own catalog stage");
    }
    let thread_cassie = cassie.clone();
    let thread_session = waiter.clone();
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let result = thread_cassie.execute_sql(&thread_session, "INSERT INTO waiting_identity (payload) VALUES ('held') ON CONFLICT (payload) DO NOTHING RETURNING id", Vec::new());
        send.send(result).expect("receive result");
    });
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    let mut early = None;
    let mut waiting = false;
    while Instant::now() < deadline {
        if cassie.transaction_unique_waits.waiting_count() == 1 {
            waiting = true;
            break;
        }
        if let Ok(result) = receive.try_recv() {
            early = Some(result);
            break;
        }
        std::thread::yield_now();
    }

    // Act
    cassie
        .execute_sql(&owner, "ROLLBACK", Vec::new())
        .expect("release actual owner");
    let inserted = early.unwrap_or_else(|| {
        receive
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("waiter retires")
    });
    worker.join().expect("worker");
    let following = cassie
        .execute_sql(
            &waiter,
            "INSERT INTO waiting_identity (payload) VALUES ('next') RETURNING id",
            Vec::new(),
        )
        .expect("following default");
    cassie
        .execute_sql(&waiter, "COMMIT", Vec::new())
        .expect("publish own metadata and rows");

    // Assert
    assert!(
        waiting,
        "the actual unique-owner edge must be installed before release"
    );
    assert_eq!(
        inserted.expect("rolled-back owner frees key").rows(),
        &[vec![Value::Int64(2)]]
    );
    assert_eq!(following.rows(), &[vec![Value::Int64(3)]]);
    assert_eq!(cassie.transaction_unique_waits.waiting_count(), 0);
}
