use super::*;

type AutocommitInsertResult = (
    tokio_postgres::Client,
    Result<tokio_postgres::Row, tokio_postgres::Error>,
);

struct PendingAutocommitInsert {
    query: tokio::task::JoinHandle<AutocommitInsertResult>,
    early_completion: Option<Result<AutocommitInsertResult, tokio::task::JoinError>>,
    request_entered_engine: bool,
}

impl PendingAutocommitInsert {
    fn returned_before_owner_resolution(&self) -> bool {
        self.early_completion.is_some()
    }

    async fn finish(self) -> AutocommitInsertResult {
        match self.early_completion {
            Some(result) => result.expect("autocommit insert task should complete"),
            None => self
                .query
                .await
                .expect("autocommit insert should finish after owner resolution"),
        }
    }
}

async fn start_autocommit_insert(
    cassie: &cassie::app::Cassie,
    contender: tokio_postgres::Client,
    sql: &'static str,
) -> PendingAutocommitInsert {
    let mut query = tokio::spawn(async move {
        let result = contender.query_one(sql, &[]).await;
        (contender, result)
    });
    let owner_wait = crate::support_unique_conflict::observe_owner_wait(cassie, &mut query).await;
    PendingAutocommitInsert {
        query,
        early_completion: owner_wait.early_completion,
        request_entered_engine: owner_wait.request_entered_engine,
    }
}

#[test]
fn should_wait_for_autocommit_insert_until_owner_commits() {
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
                "CREATE TABLE compat_unique_autocommit_commit (id INT PRIMARY KEY, value TEXT UNIQUE)",
            )
            .await
            .expect("create a unique-key transaction fixture");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the unique-key owner transaction");
        owner
            .query_one(
                "INSERT INTO compat_unique_autocommit_commit VALUES (1, 'shared') RETURNING id",
                &[],
            )
            .await
            .expect("hold the unique value in the open transaction");
        let pending = start_autocommit_insert(
            &cassie,
            contender,
            "INSERT INTO compat_unique_autocommit_commit VALUES (2, 'shared') RETURNING id",
        )
        .await;
        let request_entered_engine = pending.request_entered_engine;
        let returned_before_owner_commit = pending.returned_before_owner_resolution();

        // Act
        owner
            .batch_execute("COMMIT")
            .await
            .expect("commit the transaction that first held the unique value");
        let (contender, insert_result) = pending.finish().await;
        let error_code = insert_result
            .as_ref()
            .err()
            .and_then(tokio_postgres::Error::as_db_error)
            .map(|error| error.code().code().to_owned());
        let committed = owner
            .query("SELECT id FROM compat_unique_autocommit_commit ORDER BY id", &[])
            .await;

        // Assert
        drop((owner, contender));
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert!(request_entered_engine, "autocommit INSERT must enter the engine");
        assert!(
            !returned_before_owner_commit,
            "autocommit INSERT must wait for the explicit owner"
        );
        assert_eq!(error_code.as_deref(), Some("23505"));
        let committed = committed.expect("read rows after the owner commits");
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].get::<_, i32>(0), 1);
    });
}

#[test]
fn should_wait_for_autocommit_insert_until_owner_rolls_back() {
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
                "CREATE TABLE compat_unique_autocommit_rollback (id INT PRIMARY KEY, value TEXT UNIQUE)",
            )
            .await
            .expect("create a unique-key transaction fixture");
        execute_statements(
            &owner,
            &[
                "BEGIN",
                "INSERT INTO compat_unique_autocommit_rollback VALUES (1, 'shared')",
            ],
        )
        .await
        .expect("hold the unique value in the open transaction");
        let pending = start_autocommit_insert(
            &cassie,
            contender,
            "INSERT INTO compat_unique_autocommit_rollback VALUES (2, 'shared') RETURNING id",
        )
        .await;
        let request_entered_engine = pending.request_entered_engine;
        let returned_before_owner_rollback = pending.returned_before_owner_resolution();

        // Act
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction that first held the unique value");
        let (contender, insert_result) = pending.finish().await;
        let committed = owner
            .query("SELECT id FROM compat_unique_autocommit_rollback ORDER BY id", &[])
            .await;

        // Assert
        drop((owner, contender));
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert!(request_entered_engine, "autocommit INSERT must enter the engine");
        assert!(
            !returned_before_owner_rollback,
            "autocommit INSERT must wait for the explicit owner"
        );
        let inserted = insert_result.expect("owner rollback releases the unique value");
        assert_eq!(inserted.get::<_, i32>(0), 2);
        let committed = committed.expect("read rows after the autocommit insert finishes");
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].get::<_, i32>(0), 2);
    });
}
