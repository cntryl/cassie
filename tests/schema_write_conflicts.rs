use std::sync::{Arc, Barrier};

use cassie::app::{Cassie, CassieError};
use cassie::midge::adapter::{
    schema_write_conflict_test_guard, set_schema_write_commit_barriers, SchemaWritePausePoint,
};

#[path = "support/sql.rs"]
mod support;

#[test]
fn should_abort_one_conflicting_table_create_before_reusing_object_id() {
    // Arrange
    support::use_local_storage();
    let _test_guard = schema_write_conflict_test_guard();
    let path = support::data_dir("schema-write-conflicts");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
    cassie.startup().expect("start Cassie");
    let ready = Arc::new(Barrier::new(3));
    let resume = Arc::new(Barrier::new(3));
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::CollectionCreate),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let statements = [
        (
            "schema_conflict_alpha",
            "CREATE TABLE schema_conflict_alpha (value TEXT)",
        ),
        (
            "schema_conflict_beta",
            "CREATE TABLE schema_conflict_beta (value TEXT)",
        ),
    ];
    let workers = statements.map(|(table, sql)| {
        let worker_cassie = Arc::clone(&cassie);
        std::thread::spawn(move || {
            let session = worker_cassie.create_session("tester", None);
            (table, sql, worker_cassie.execute_sql(&session, sql, vec![]))
        })
    });
    ready.wait();
    set_schema_write_commit_barriers(None, None, None);

    // Act
    resume.wait();
    let outcomes = workers.map(|worker| worker.join().expect("join schema worker"));
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
fn should_abort_one_conflicting_nextval_before_returning_a_duplicate_value() {
    // Arrange
    support::use_local_storage();
    let _test_guard = schema_write_conflict_test_guard();
    let path = support::data_dir("sequence-write-conflicts");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
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
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::SequenceNextValue),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let workers = [(), ()].map(|()| {
        let worker_cassie = Arc::clone(&cassie);
        std::thread::spawn(move || {
            worker_cassie
                .midge
                .next_sequence_value("sequence_conflict_ids")
        })
    });
    ready.wait();
    set_schema_write_commit_barriers(None, None, None);

    // Act
    resume.wait();
    let outcomes = workers.map(|worker| worker.join().expect("join sequence worker"));
    let mut returned_ids = Vec::new();
    let mut retryable_conflicts = 0_usize;
    for result in outcomes {
        match result {
            Ok(value) => returned_ids.push(value),
            Err(CassieError::StorageRetryable(message))
                if message
                    .to_ascii_lowercase()
                    .starts_with("midge write conflict") =>
            {
                retryable_conflicts += 1;
            }
            Err(error) => panic!("unexpected sequence error: {error}"),
        }
    }
    assert_eq!(returned_ids.len(), 1, "one nextval call should commit");
    assert_eq!(retryable_conflicts, 1, "one nextval call should abort");
    returned_ids.push(
        cassie
            .midge
            .next_sequence_value("sequence_conflict_ids")
            .expect("retry nextval"),
    );

    // Assert
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
    support::use_local_storage();
    let _test_guard = schema_write_conflict_test_guard();
    let path = support::data_dir("database-write-conflicts");
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("create Cassie"));
    cassie.startup().expect("start Cassie");
    let ready = Arc::new(Barrier::new(3));
    let resume = Arc::new(Barrier::new(3));
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::DatabaseCreateFinalize),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let workers = ["database_conflict_alpha", "database_conflict_beta"].map(|database| {
        let worker_cassie = Arc::clone(&cassie);
        std::thread::spawn(move || {
            (
                database,
                worker_cassie.midge.create_database(database, None),
            )
        })
    });
    ready.wait();
    set_schema_write_commit_barriers(None, None, None);

    // Act
    resume.wait();
    let outcomes = workers.map(|worker| worker.join().expect("join database worker"));
    let mut successes = 0_usize;
    let mut losing_database = None;
    for (database, result) in outcomes {
        match result {
            Ok(()) => successes += 1,
            Err(CassieError::StorageRetryable(message))
                if message
                    .to_ascii_lowercase()
                    .starts_with("midge write conflict") =>
            {
                losing_database = Some(database);
            }
            Err(error) => panic!("unexpected database creation error: {error}"),
        }
    }
    assert_eq!(successes, 1, "one database creation should commit");
    cassie
        .midge
        .create_database(losing_database.expect("one losing database creation"), None)
        .expect("retry database creation");
    let names_before_restart = cassie
        .midge
        .list_databases()
        .expect("list databases before restart")
        .into_iter()
        .map(|database| database.name)
        .collect::<Vec<_>>();
    assert!(names_before_restart.contains(&"database_conflict_alpha".to_string()));
    assert!(names_before_restart.contains(&"database_conflict_beta".to_string()));
    drop(cassie);
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("restart Cassie");

    // Assert
    let names_after_restart = restarted
        .midge
        .list_databases()
        .expect("list databases after restart")
        .into_iter()
        .map(|database| database.name)
        .collect::<Vec<_>>();
    assert!(names_after_restart.contains(&"database_conflict_alpha".to_string()));
    assert!(names_after_restart.contains(&"database_conflict_beta".to_string()));
    assert!(restarted.catalog.database_exists("database_conflict_alpha"));
    assert!(restarted.catalog.database_exists("database_conflict_beta"));

    drop(restarted);
    let _ = std::fs::remove_dir_all(path);
}
