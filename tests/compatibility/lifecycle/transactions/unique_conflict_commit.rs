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

#[test]
fn should_report_update_owned_unique_conflict_fields_and_abort_waiting_transaction() {
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
                "CREATE TABLE compat_update_unique_wait_rows \
                 (id INT PRIMARY KEY, value TEXT UNIQUE)",
            )
            .await
            .expect("create an UPDATE-owned unique-key fixture");
        owner
            .batch_execute(
                "INSERT INTO compat_update_unique_wait_rows VALUES (1, 'original')",
            )
            .await
            .expect("seed the row that the owner will update");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction that stages the unique value");
        owner
            .batch_execute(
                "UPDATE compat_update_unique_wait_rows SET value = 'shared' WHERE id = 1",
            )
            .await
            .expect("stage the unique value in an UPDATE");
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the waiting transaction");

        let mut contender_insert = tokio::spawn(async move {
            let result = contender
                .query_one(
                    "INSERT INTO compat_update_unique_wait_rows VALUES (2, 'shared') RETURNING id",
                    &[],
                )
                .await;
            (contender, result)
        });

        // Act
        let owner_wait =
            crate::support_unique_conflict::observe_owner_wait(&cassie, &mut contender_insert)
                .await;
        let request_entered_engine = owner_wait.request_entered_engine;
        let early_completion = owner_wait.early_completion;
        let completed_before_owner_commit = early_completion.is_some();
        owner
            .batch_execute("COMMIT")
            .await
            .expect("commit the transaction that first staged the unique value");
        let (contender, insert_result) = match early_completion {
            Some(result) => result.expect("waiting INSERT task should finish"),
            None => contender_insert
                .await
                .expect("waiting INSERT should finish after owner commit"),
        };
        let insert_error = insert_result
            .as_ref()
            .expect_err("the waiting INSERT should report the committed duplicate");
        let insert_db_error = insert_error
            .as_db_error()
            .expect("the unique conflict should be a PostgreSQL error");
        let sqlstate = insert_db_error.code().code().to_owned();
        let message = insert_db_error.message().to_owned();
        let constraint = insert_db_error.constraint().map(str::to_owned);
        let detail = insert_db_error.detail().map(str::to_owned);
        let following_statement = contender.query_one("SELECT 1", &[]).await;
        let following_sqlstate = following_statement
            .as_ref()
            .err()
            .and_then(tokio_postgres::Error::as_db_error)
            .map(|error| error.code().code().to_owned());
        let contender_commit = contender.batch_execute("COMMIT").await;

        // Assert
        drop((owner, contender));
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert!(request_entered_engine, "waiting INSERT must enter the engine");
        assert!(
            !completed_before_owner_commit,
            "INSERT must wait for owner resolution"
        );
        assert_eq!(sqlstate, "23505");
        assert_eq!(
            message,
            "duplicate key value violates unique constraint \"compat_update_unique_wait_rows_value_key\""
        );
        assert_eq!(
            constraint.as_deref(),
            Some("compat_update_unique_wait_rows_value_key")
        );
        assert_eq!(
            detail.as_deref(),
            Some("Key (value)=(shared) already exists.")
        );
        assert_eq!(following_sqlstate.as_deref(), Some("25P02"));
        contender_commit.expect("COMMIT should finish the failed transaction as a rollback");
    });
}

#[test]
fn should_report_other_unique_key_after_wire_insert_wait_and_abort_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let crate::support_unique_conflict::Fixture {
            cassie,
            owner: first_owner,
            contender,
            owner_connection,
            contender_connection,
            server,
        } = crate::support_unique_conflict::start().await;
        first_owner
            .batch_execute(
                "CREATE TABLE compat_unique_recheck_rows \
                 (a INT UNIQUE, b INT UNIQUE)",
            )
            .await
            .expect("create a two-key unique fixture");
        first_owner
            .batch_execute("BEGIN; INSERT INTO compat_unique_recheck_rows VALUES (1, 11)")
            .await
            .expect("hold a=1 in the first wire transaction");
        let second_owner = cassie.create_session("issue_1466_second_owner", None);
        cassie
            .execute_sql(&second_owner, "BEGIN", Vec::new())
            .expect("begin the independent second-key transaction");
        cassie
            .execute_sql(
                &second_owner,
                "INSERT INTO compat_unique_recheck_rows VALUES (9, 22)",
                Vec::new(),
            )
            .expect("stage b=2 in the second-key transaction");
        cassie
            .execute_sql(&second_owner, "COMMIT", Vec::new())
            .expect("commit b=2 before the first owner resolves");
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the wire transaction that waits on a=1");

        let mut contender_insert = tokio::spawn(async move {
            let result = contender
                .query_one(
                    "INSERT INTO compat_unique_recheck_rows \
                     VALUES (1, 11), (2, 22) RETURNING a",
                    &[],
                )
                .await;
            (contender, result)
        });

        // Act
        let owner_wait =
            crate::support_unique_conflict::observe_owner_wait(&cassie, &mut contender_insert)
                .await;
        let request_entered_engine = owner_wait.request_entered_engine;
        let early_completion = owner_wait.early_completion;
        let completed_before_owner_rollback = early_completion.is_some();
        first_owner
            .batch_execute("ROLLBACK")
            .await
            .expect("release a=1 from the first owner");
        let (contender, insert_result) = match early_completion {
            Some(result) => result.expect("waiting INSERT task should finish"),
            None => contender_insert
                .await
                .expect("waiting INSERT should finish after owner rollback"),
        };
        let insert_error = insert_result
            .as_ref()
            .expect_err("the resumed INSERT should find committed b=2");
        let insert_db_error = insert_error
            .as_db_error()
            .expect("the conflicting key should be a PostgreSQL error");
        let sqlstate = insert_db_error.code().code().to_owned();
        let message = insert_db_error.message().to_owned();
        let constraint = insert_db_error.constraint().map(str::to_owned);
        let detail = insert_db_error.detail().map(str::to_owned);
        let following_statement = contender.query_one("SELECT 1", &[]).await;
        let following_sqlstate = following_statement
            .as_ref()
            .err()
            .and_then(tokio_postgres::Error::as_db_error)
            .map(|error| error.code().code().to_owned());
        let contender_commit = contender.batch_execute("COMMIT").await;

        // Assert
        drop((first_owner, contender, second_owner));
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert!(request_entered_engine, "waiting INSERT must enter the engine");
        assert!(
            !completed_before_owner_rollback,
            "INSERT must wait for the first owner to resolve"
        );
        assert_eq!(sqlstate, "23505");
        assert_eq!(
            message,
            "duplicate key value violates unique constraint \"compat_unique_recheck_rows_b_key\""
        );
        assert_eq!(
            constraint.as_deref(),
            Some("compat_unique_recheck_rows_b_key")
        );
        assert_eq!(detail.as_deref(), Some("Key (b)=(22) already exists."));
        assert_eq!(following_sqlstate.as_deref(), Some("25P02"));
        contender_commit.expect("COMMIT should finish the failed transaction as a rollback");
    });
}

#[test]
fn should_rollback_modifying_cte_side_effect_when_resumed_insert_conflicts() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let crate::support_unique_conflict::Fixture {
            cassie,
            owner: first_owner,
            contender,
            owner_connection,
            contender_connection,
            server,
        } = crate::support_unique_conflict::start().await;
        first_owner
            .batch_execute(
                "CREATE TABLE compat_unique_cte_rows (a INT UNIQUE, b INT UNIQUE); \
                 CREATE TABLE compat_unique_cte_mark (id INT PRIMARY KEY)",
            )
            .await
            .expect("create the unique target and side-effect tables");
        first_owner
            .batch_execute("BEGIN; INSERT INTO compat_unique_cte_rows VALUES (1, 100)")
            .await
            .expect("hold a=1 in the first wire transaction");
        let second_owner = cassie.create_session("issue_1466_cte_second_owner", None);
        cassie
            .execute_sql(&second_owner, "BEGIN", Vec::new())
            .expect("begin the independent second-key transaction");
        cassie
            .execute_sql(
                &second_owner,
                "INSERT INTO compat_unique_cte_rows VALUES (9, 2)",
                Vec::new(),
            )
            .expect("stage b=2 in the second-key transaction");
        cassie
            .execute_sql(&second_owner, "COMMIT", Vec::new())
            .expect("commit b=2 before the first owner resolves");
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the wire transaction that waits on a=1");

        let mut contender_insert = tokio::spawn(async move {
            let result = contender
                .query_one(
                    "WITH side AS (\
                         INSERT INTO compat_unique_cte_mark VALUES (1) RETURNING id\
                     ) \
                     INSERT INTO compat_unique_cte_rows VALUES (1, 2) RETURNING a",
                    &[],
                )
                .await;
            (contender, result)
        });

        // Act
        let owner_wait =
            crate::support_unique_conflict::observe_owner_wait(&cassie, &mut contender_insert)
                .await;
        let request_entered_engine = owner_wait.request_entered_engine;
        let early_completion = owner_wait.early_completion;
        let completed_before_owner_rollback = early_completion.is_some();
        first_owner
            .batch_execute("ROLLBACK")
            .await
            .expect("release a=1 from the first owner");
        let (contender, insert_result) = match early_completion {
            Some(result) => result.expect("waiting INSERT task should finish"),
            None => contender_insert
                .await
                .expect("waiting INSERT should finish after owner rollback"),
        };
        let insert_error = insert_result
            .as_ref()
            .expect_err("the resumed INSERT should find committed b=2");
        let insert_db_error = insert_error
            .as_db_error()
            .expect("the conflicting key should be a PostgreSQL error");
        let sqlstate = insert_db_error.code().code().to_owned();
        let constraint = insert_db_error.constraint().map(str::to_owned);
        let detail = insert_db_error.detail().map(str::to_owned);
        let following_statement = contender.query_one("SELECT 1", &[]).await;
        let following_sqlstate = following_statement
            .as_ref()
            .err()
            .and_then(tokio_postgres::Error::as_db_error)
            .map(|error| error.code().code().to_owned());
        let contender_commit = contender.batch_execute("COMMIT").await;
        let marked_rows: i64 = first_owner
            .query_one("SELECT count(*) FROM compat_unique_cte_mark", &[])
            .await
            .expect("read the CTE side-effect table after rollback")
            .get(0);

        // Assert
        drop((first_owner, contender, second_owner));
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert!(request_entered_engine, "waiting INSERT must enter the engine");
        assert!(
            !completed_before_owner_rollback,
            "INSERT must wait for the first owner to resolve"
        );
        assert_eq!(sqlstate, "23505");
        assert_eq!(
            constraint.as_deref(),
            Some("compat_unique_cte_rows_b_key")
        );
        assert_eq!(detail.as_deref(), Some("Key (b)=(2) already exists."));
        assert_eq!(following_sqlstate.as_deref(), Some("25P02"));
        contender_commit.expect("COMMIT should finish the failed transaction as a rollback");
        assert_eq!(marked_rows, 0, "failed outer INSERT must roll back its CTE write");
    });
}
