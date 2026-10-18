use super::*;

struct UniqueConflictRollbackObservations {
    request_entered_engine: bool,
    returned_before_owner_rollback: bool,
    insert_outcome: String,
    insert_result: Result<tokio_postgres::Row, tokio_postgres::Error>,
    committed: Result<(), tokio_postgres::Error>,
    stored: Result<Vec<tokio_postgres::Row>, tokio_postgres::Error>,
}

fn assert_unique_conflict_rollback(observations: UniqueConflictRollbackObservations) {
    assert!(
        !observations.returned_before_owner_rollback,
        "PostgreSQL waits for the unresolved unique-key owner before returning INSERT; \
         engine entry observed: {}; early result: {}",
        observations.request_entered_engine, observations.insert_outcome
    );
    let inserted = observations
        .insert_result
        .expect("owner rollback makes the competing insert succeed");
    assert_eq!(inserted.get::<_, i32>(0), 2);
    observations
        .committed
        .expect("COMMIT after the owner rollback should succeed");
    let stored = observations
        .stored
        .expect("read rows after the owner rollback");
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].get::<_, i32>(0), 2);
}

#[test]
fn should_wait_for_unique_conflict_owner_rollback_before_returning_insert() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        support::use_local_storage();
        let path = support::data_dir("unique_conflict_owner_rollback");
        let _directory = DataDirectory(path.clone());
        let crate::support_unique_conflict::Fixture {
            cassie,
            owner,
            contender,
            owner_connection,
            contender_connection,
            server,
        } = crate::support_unique_conflict::start(&path).await;
        owner
            .batch_execute(
                "CREATE TABLE compat_unique_rollback_rows (id INT PRIMARY KEY, value TEXT UNIQUE)",
            )
            .await
            .expect("create a unique-key transaction fixture");
        execute_statements(
            &owner,
            &[
                "BEGIN",
                "INSERT INTO compat_unique_rollback_rows VALUES (1, 'shared')",
            ],
        )
        .await
        .expect("hold the first unique value in an open transaction");
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the competing transaction");

        let mut contender_insert = tokio::spawn(async move {
            let result = contender
                .query_one(
                    "INSERT INTO compat_unique_rollback_rows VALUES (2, 'shared') RETURNING id",
                    &[],
                )
                .await;
            (contender, result)
        });

        // Act
        // Confirm the conflicting request entered Cassie before observing whether it returns.
        let owner_wait =
            crate::support_unique_conflict::observe_owner_wait(&cassie, &mut contender_insert)
                .await;
        let request_entered_engine = owner_wait.request_entered_engine;
        let early_completion = owner_wait.early_completion;
        let returned_before_owner_rollback = early_completion.is_some();
        assert!(
            request_entered_engine || returned_before_owner_rollback,
            "the competing query must reach the engine or complete before owner rollback"
        );

        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction that first held the unique value");
        let (contender, insert_result) = match early_completion {
            Some(result) => result.expect("competing insert task should complete"),
            None => contender_insert
                .await
                .expect("competing insert task should complete after owner rollback"),
        };
        let committed = contender.batch_execute("COMMIT").await;
        let stored = owner
            .query(
                "SELECT id FROM compat_unique_rollback_rows ORDER BY id",
                &[],
            )
            .await;
        let insert_outcome = match &insert_result {
            Ok(row) => format!("Ok({})", row.get::<_, i32>(0)),
            Err(error) => format!("Err({error})"),
        };

        // Assert
        drop((owner, contender));
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert_unique_conflict_rollback(UniqueConflictRollbackObservations {
            request_entered_engine,
            returned_before_owner_rollback,
            insert_outcome,
            insert_result,
            committed,
            stored,
        });
    });
}
