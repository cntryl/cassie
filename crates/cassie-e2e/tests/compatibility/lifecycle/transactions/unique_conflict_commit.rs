use super::*;

struct UniqueConflictCommitObservations {
    request_entered_engine: bool,
    completed_before_owner_commit: bool,
    insert_error_code: Option<String>,
    returned_id: Option<i32>,
    contender_commit: Result<(), tokio_postgres::Error>,
    committed_rows: Result<Vec<tokio_postgres::Row>, tokio_postgres::Error>,
}

fn assert_unique_conflict_commit(observations: UniqueConflictCommitObservations) {
    assert!(
        observations.request_entered_engine || observations.completed_before_owner_commit,
        "the conflicting INSERT must reach the engine or complete before owner commit"
    );
    assert!(
        !observations.completed_before_owner_commit,
        "PostgreSQL waits for the unresolved unique-key owner before returning INSERT ... RETURNING"
    );
    assert_eq!(
        observations.insert_error_code.as_deref(),
        Some("23505"),
        "the contender INSERT itself must report unique_violation after the owner commits; returned row: {:?}",
        observations.returned_id
    );
    assert!(
        observations.contender_commit.is_ok(),
        "COMMIT after a failed INSERT must finish as ROLLBACK, not report the conflict at commit: {:?}",
        observations.contender_commit
    );
    let committed_rows = observations
        .committed_rows
        .expect("read rows after both transactions finish");
    assert_eq!(committed_rows.len(), 1);
    assert_eq!(committed_rows[0].get::<_, i32>(0), 1);
}

#[test]
fn should_wait_for_unique_conflict_owner_commit_before_returning_unique_violation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let crate::support_unique_conflict::Fixture {
            cassie,
            owner,
            contender,
            owner_connection,
            contender_connection,
            server,
        } = crate::support_unique_conflict::start().await;
        owner
            .batch_execute(
                "CREATE TABLE compat_unique_commit_rows (id INT PRIMARY KEY, value TEXT UNIQUE)",
            )
            .await
            .expect("create a unique-key transaction fixture");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the first transaction");
        let owner_insert = owner
            .query_one(
                "INSERT INTO compat_unique_commit_rows VALUES (1, 'shared') RETURNING id",
                &[],
            )
            .await
            .expect("hold the first unique value in an open transaction");
        assert_eq!(owner_insert.get::<_, i32>(0), 1);
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the competing transaction");

        let mut contender_insert = tokio::spawn(async move {
            let result = contender
                .query_one(
                    "INSERT INTO compat_unique_commit_rows VALUES (2, 'shared') RETURNING id",
                    &[],
                )
                .await;
            (contender, result)
        });

        // Act
        // Observe engine entry before measuring whether INSERT ... RETURNING completes.
        let owner_wait =
            crate::support_unique_conflict::observe_owner_wait(&cassie, &mut contender_insert)
                .await;
        let request_entered_engine = owner_wait.request_entered_engine;
        let early_completion = owner_wait.early_completion;
        let completed_before_owner_commit = early_completion.is_some();

        owner
            .batch_execute("COMMIT")
            .await
            .expect("commit the transaction that first held the unique value");
        let (contender, insert_result) = match early_completion {
            Some(result) => result.expect("competing insert task should complete"),
            None => contender_insert
                .await
                .expect("competing insert should complete after owner commit"),
        };
        let insert_error_code = insert_result
            .as_ref()
            .err()
            .and_then(|error| error.as_db_error())
            .map(|error| error.code().code().to_string());
        let returned_id = insert_result.as_ref().ok().map(|row| row.get::<_, i32>(0));
        let contender_commit = contender.batch_execute("COMMIT").await;
        if contender_commit.is_err() {
            let _ = contender.batch_execute("ROLLBACK").await;
        }
        let committed_rows = owner
            .query("SELECT id FROM compat_unique_commit_rows ORDER BY id", &[])
            .await;

        // Assert
        drop((owner, contender));
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert_unique_conflict_commit(UniqueConflictCommitObservations {
            request_entered_engine,
            completed_before_owner_commit,
            insert_error_code,
            returned_id,
            contender_commit,
            committed_rows,
        });
    });
}
