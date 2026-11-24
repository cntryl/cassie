use super::support_sql as support;

use std::sync::{Arc, Barrier};

use cassie::app::{Cassie, CassieError, StorageRetryKind};
use cassie::catalog::DatabaseMeta;
use cassie::config::CassieRuntimeConfig;
use cassie::midge::adapter::{
    schema_write_conflict_test_guard, schema_write_conflict_worker_guard,
    set_schema_write_commit_barriers, SchemaWritePausePoint,
};
use cntryl_midge::{TransactionMode, WriteOptions};

#[test]
fn should_abort_one_conflicting_table_create_before_reusing_object_id() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    let path = support::data_dir("schema-write-conflicts");
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("create Cassie"));
    cassie.startup().expect("start Cassie");
    let ready = Arc::new(Barrier::new(3));
    let resume = Arc::new(Barrier::new(3));
    let mut cleanup = cassie::runtime::test_sync::TestBarrierCleanup::arm([
        Arc::clone(&ready),
        Arc::clone(&resume),
    ]);
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::CollectionCreate),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let statements = [
        (
            "schema_conflict_alpha",
            "CREATE TABLE schema_conflict_alpha (id INT PRIMARY KEY, value TEXT)",
        ),
        (
            "schema_conflict_beta",
            "CREATE TABLE schema_conflict_beta (id INT PRIMARY KEY, value TEXT)",
        ),
    ];
    let workers = statements.map(|(table, sql)| {
        let worker_cassie = Arc::clone(&cassie);
        std::thread::spawn(move || {
            let _worker_guard = schema_write_conflict_worker_guard();
            let session = worker_cassie.create_session("tester", None);
            (table, sql, worker_cassie.execute_sql(&session, sql, vec![]))
        })
    });
    cassie::runtime::test_sync::wait_for_test_barrier(&ready, "collection create ready");
    set_schema_write_commit_barriers(None, None, None);

    // Act
    cassie::runtime::test_sync::wait_for_test_barrier(&resume, "collection create resume");
    let outcomes =
        workers.map(|worker| cassie::runtime::test_sync::join_test_thread(worker, "schema worker"));
    cleanup.disarm();
    let mut successes = 0_usize;
    let mut retryable_conflicts = 0_usize;
    let mut losing_statement = None;
    for (_, sql, result) in outcomes {
        match result {
            Ok(_) => successes += 1,
            Err(CassieError::StorageRetryable(message))
                if message
                    .to_ascii_lowercase()
                    .starts_with("midge write conflict") =>
            {
                retryable_conflicts += 1;
                losing_statement = Some(sql);
            }
            Err(error) => panic!("unexpected schema creation error: {error}"),
        }
    }
    assert_eq!(successes, 1, "exactly one conflicting DDL should commit");
    assert_eq!(
        retryable_conflicts, 1,
        "exactly one conflicting DDL should abort"
    );
    let retry_session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &retry_session,
            losing_statement.expect("one losing schema statement"),
            vec![],
        )
        .expect("retry conflicting schema statement");
    let object_ids = statements.map(|(table, _)| {
        cassie
            .midge
            .collection_metadata(table)
            .expect("read collection metadata")
            .expect("created collection metadata")
            .storage_id
    });

    // Assert
    assert!(object_ids.iter().all(|object_id| *object_id > 0));
    assert_ne!(object_ids[0], object_ids[1]);

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_retry_one_conflicting_nextval_before_returning_unique_values() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    let path = support::data_dir("sequence-write-conflicts");
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("create Cassie"));
    cassie.startup().expect("start Cassie");
    let setup_session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &setup_session,
            "CREATE SEQUENCE sequence_conflict_ids",
            vec![],
        )
        .expect("create sequence");
    let ready = Arc::new(Barrier::new(3));
    let resume = Arc::new(Barrier::new(3));
    let mut cleanup = cassie::runtime::test_sync::TestBarrierCleanup::arm([
        Arc::clone(&ready),
        Arc::clone(&resume),
    ]);
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::SequenceNextValue),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let workers = [(), ()].map(|()| {
        let worker_cassie = Arc::clone(&cassie);
        std::thread::spawn(move || {
            let _worker_guard = schema_write_conflict_worker_guard();
            worker_cassie
                .midge
                .next_sequence_value("sequence_conflict_ids")
        })
    });
    cassie::runtime::test_sync::wait_for_test_barrier(&ready, "sequence next-value ready");
    set_schema_write_commit_barriers(None, None, None);

    // Act
    cassie::runtime::test_sync::wait_for_test_barrier(&resume, "sequence next-value resume");
    let outcomes = workers
        .map(|worker| cassie::runtime::test_sync::join_test_thread(worker, "sequence worker"));
    cleanup.disarm();
    let mut returned_ids = Vec::with_capacity(outcomes.len());
    for result in outcomes {
        match result {
            Ok(value) => returned_ids.push(value),
            Err(error) => panic!("nextval should retry storage conflicts: {error}"),
        }
    }

    // Assert
    assert_eq!(returned_ids.len(), 2, "both nextval calls should complete");
    returned_ids.sort_unstable();
    assert_eq!(returned_ids, vec![1, 2]);
    let stored = cassie
        .midge
        .get_sequence("sequence_conflict_ids")
        .expect("read sequence")
        .expect("stored sequence");
    assert_eq!(stored.current_value, 2);

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_concurrent_database_creates_across_retry_restart() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    let path = support::data_dir("database-write-conflicts");
    let open = || {
        Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default())
            .expect("open local Cassie")
    };
    let cassie = Arc::new(open());
    cassie.startup().expect("start Cassie");
    let ready = Arc::new(Barrier::new(3));
    let resume = Arc::new(Barrier::new(3));
    let mut cleanup = cassie::runtime::test_sync::TestBarrierCleanup::arm([
        Arc::clone(&ready),
        Arc::clone(&resume),
    ]);
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::DatabaseCreateFinalize),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let workers = ["database_conflict_alpha", "database_conflict_beta"].map(|database| {
        let worker_cassie = Arc::clone(&cassie);
        std::thread::spawn(move || {
            let worker_guard = schema_write_conflict_worker_guard();
            let result = worker_cassie.midge.create_database(database, None);
            worker_guard.complete(SchemaWritePausePoint::DatabaseCreateFinalize);
            (database, result)
        })
    });
    cassie::runtime::test_sync::wait_for_test_barrier(&ready, "database create ready");
    set_schema_write_commit_barriers(None, None, None);

    // Act
    cassie::runtime::test_sync::wait_for_test_barrier(&resume, "database create resume");
    let outcomes = workers
        .map(|worker| cassie::runtime::test_sync::join_test_thread(worker, "database worker"));
    cleanup.disarm();
    let (winner, loser, error) = match outcomes {
        [(winner, Ok(())), (loser, Err(error))] | [(loser, Err(error)), (winner, Ok(()))] => {
            (winner, loser, error)
        }
        other => panic!("expected one successful create and one conflict or stall: {other:?}"),
    };
    let CassieError::StorageRetryable(retry) = error else {
        panic!("unexpected database creation error: {error}");
    };
    assert!(
        matches!(
            retry.kind(),
            StorageRetryKind::WriteConflict | StorageRetryKind::WriteStall
        ),
        "unexpected retryable error: {retry}"
    );
    eprintln!(
        "concurrent create: {winner} succeeded, {loser} returned {:?}",
        retry.kind()
    );
    assert!(cassie.midge.get_database(loser).unwrap().is_none());
    assert!(matches!(
        cassie.midge.database_family(loser),
        Err(CassieError::NotFound(_))
    ));

    let winner_owner = write_database_sentinel(&cassie, winner);
    cassie
        .midge
        .create_database(loser, None)
        .expect("retry losing create");
    let loser_owner = write_database_sentinel(&cassie, loser);
    assert_ne!(winner_owner.physical_family, loser_owner.physical_family);
    let owners = [winner_owner, loser_owner];
    assert_database_sentinels(&cassie, &owners);
    let registry = cassie.midge.list_databases().expect("read live registry");
    assert!(owners.iter().all(|owner| registry.contains(owner)));
    drop(cassie);
    let restarted = open();
    restarted.startup().expect("restart Cassie");

    // Assert
    assert_eq!(restarted.midge.list_databases().unwrap(), registry);
    for owner in &owners {
        assert_eq!(
            restarted.catalog.get_database(&owner.name).as_ref(),
            Some(owner)
        );
    }
    assert_database_sentinels(&restarted, &owners);
    drop(restarted);
    std::fs::remove_dir_all(path).expect("remove reopened local store");
}

const DATABASE_SENTINEL: &[u8] = b"database-create-retry-sentinel";

fn write_database_sentinel(cassie: &Cassie, database: &str) -> DatabaseMeta {
    let owner = cassie
        .midge
        .get_database(database)
        .unwrap()
        .expect("registered owner");
    assert_eq!(owner.name, database);
    let mut tx = cassie
        .midge
        .database_tx(database, TransactionMode::ReadWrite)
        .expect("open database for sentinel");
    tx.put(
        DATABASE_SENTINEL.to_vec(),
        database.as_bytes().to_vec(),
        None,
    )
    .unwrap();
    tx.commit(WriteOptions::sync()).expect("persist sentinel");
    owner
}

fn assert_database_sentinels(cassie: &Cassie, owners: &[DatabaseMeta]) {
    for owner in owners {
        let family = cassie
            .midge
            .database_family(&owner.name)
            .expect("resolve physical family");
        assert_eq!(family.name(), owner.physical_family);
        assert_eq!(
            cassie
                .midge
                .raw_get_database(&owner.name, DATABASE_SENTINEL)
                .unwrap()
                .as_deref(),
            Some(owner.name.as_bytes())
        );
    }
}
