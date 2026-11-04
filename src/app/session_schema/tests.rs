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
        session.attach_schema_context(&scoped);
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
            ._owner
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
