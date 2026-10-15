use super::*;

#[test]
fn should_report_missing_prepared_gid_with_postgres_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_transaction_missing_gid").await;
        let (client, connection) = server.connect().await;
        let gid = "compat_prepared_transaction_missing_gid";

        // Act
        let resolution = client
            .batch_execute(&format!("ROLLBACK PREPARED '{gid}'"))
            .await
            .expect_err("a missing prepared transaction cannot be resolved");

        // Assert
        // PostgreSQL 18.6 returns 42704 (undefined_object) for a missing GID.
        assert_eq!(db_error(&resolution).code().code(), "42704");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_duplicate_prepared_transaction_gid_with_postgres_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_transaction_duplicate_gid").await;
        let (owner, owner_connection) = server.connect().await;
        let (duplicate, duplicate_connection) = server.connect().await;
        let gid = "compat_prepared_duplicate_gid";
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction that owns the GID");
        owner
            .batch_execute(&format!("PREPARE TRANSACTION '{gid}'"))
            .await
            .expect("prepare the first transaction with this GID");
        duplicate
            .batch_execute("BEGIN")
            .await
            .expect("begin a second transaction for the duplicate-GID check");

        // Act
        let duplicate_gid = duplicate
            .batch_execute(&format!("PREPARE TRANSACTION '{gid}'"))
            .await;
        let duplicate_sqlstate = duplicate_gid
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        if duplicate_gid.is_err() {
            duplicate
                .batch_execute("ROLLBACK")
                .await
                .expect("leave the failed duplicate-GID transaction");
        } else {
            duplicate
                .batch_execute(&format!("ROLLBACK PREPARED '{gid}'"))
                .await
                .expect("clean up a duplicate GID accepted by the server");
        }
        let owner_cleanup = owner
            .batch_execute(&format!("ROLLBACK PREPARED '{gid}'"))
            .await;

        // Assert
        drop((owner, duplicate));
        server.shutdown(owner_connection).await;
        duplicate_connection.abort();
        let _ = duplicate_connection.await;
        assert_eq!(
            duplicate_sqlstate.as_deref(),
            Some("42710"),
            "PostgreSQL rejects a duplicate prepared GID with duplicate_object"
        );
        owner_cleanup.expect("the original prepared transaction remains resolvable");
    });
}

#[test]
fn should_restrict_prepared_transaction_resolution_to_its_owner() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_transaction_owner").await;
        let (owner, owner_connection) = server.connect().await;
        let (resolver, resolver_connection) = server.connect().await;
        let gid = "compat_prepared_owner_gid";
        execute_statements(
            &owner,
            &[
                "CREATE ROLE compat_prepared_owner NOLOGIN",
                "CREATE ROLE compat_prepared_stranger NOLOGIN",
                "CREATE TABLE compat_prepared_owner_rows (id INT PRIMARY KEY)",
                "GRANT INSERT ON compat_prepared_owner_rows TO compat_prepared_owner",
            ],
        )
        .await
        .expect("create distinct prepared-transaction roles and a row fixture");
        execute_statements(
            &owner,
            &[
                "SET ROLE compat_prepared_owner",
                "BEGIN",
                "INSERT INTO compat_prepared_owner_rows VALUES (1)",
            ],
        )
        .await
        .expect("stage a row under the transaction owner's role");
        owner
            .batch_execute(&format!("PREPARE TRANSACTION '{gid}'"))
            .await
            .expect("prepare the owner's transaction");
        let metadata = resolver
            .query_one(
                &format!("SELECT owner FROM pg_catalog.pg_prepared_xacts WHERE gid = '{gid}'"),
                &[],
            )
            .await
            .expect("inspect prepared transaction ownership");
        let staged_rows: i64 = resolver
            .query_one("SELECT count(*) FROM compat_prepared_owner_rows", &[])
            .await
            .expect("verify prepared work is not visible before resolution")
            .try_get(0)
            .expect("visible row count before resolution");
        resolver
            .batch_execute("SET ROLE compat_prepared_stranger")
            .await
            .expect("assume a role that does not own the prepared transaction");

        // Act
        let unauthorized = resolver
            .batch_execute(&format!("COMMIT PREPARED '{gid}'"))
            .await;
        let unauthorized_sqlstate = unauthorized
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        resolver
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role after the denied resolution");
        let cleanup = if unauthorized.is_err() {
            Some(
                owner
                    .batch_execute(&format!("COMMIT PREPARED '{gid}'"))
                    .await,
            )
        } else {
            None
        };
        let committed_rows: i64 = resolver
            .query_one("SELECT count(*) FROM compat_prepared_owner_rows", &[])
            .await
            .expect("read the owner's committed row")
            .try_get(0)
            .expect("committed row count");

        // Assert
        drop((owner, resolver));
        server.shutdown(owner_connection).await;
        resolver_connection.abort();
        let _ = resolver_connection.await;
        assert_eq!(metadata.get::<_, String>(0), "compat_prepared_owner");
        assert_eq!(staged_rows, 0);
        assert_eq!(
            unauthorized_sqlstate.as_deref(),
            Some("42501"),
            "PostgreSQL denies prepared transaction resolution to a non-owner"
        );
        cleanup
            .expect("the owning role can commit the prepared transaction")
            .expect("owner cleanup should commit successfully");
        assert_eq!(committed_rows, 1);
    });
}

#[test]
fn should_reject_prepared_resolution_inside_a_transaction_with_postgres_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_transaction_invalid_state").await;
        let (owner, owner_connection) = server.connect().await;
        let (resolver, resolver_connection) = server.connect().await;
        let gid = "compat_prepared_invalid_state_gid";
        owner
            .batch_execute("CREATE TABLE compat_prepared_state_rows (id INT)")
            .await
            .expect("create a row fixture before preparing the transaction");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction to be prepared");
        owner
            .batch_execute("INSERT INTO compat_prepared_state_rows VALUES (1)")
            .await
            .expect("stage a row in the prepared transaction");
        owner
            .batch_execute(&format!("PREPARE TRANSACTION '{gid}'"))
            .await
            .expect("prepare an existing GID for the invalid-state probe");
        resolver
            .batch_execute("BEGIN")
            .await
            .expect("begin the resolver's ordinary transaction");

        // Act
        let invalid_state = resolver
            .batch_execute(&format!("COMMIT PREPARED '{gid}'"))
            .await;
        let invalid_sqlstate = invalid_state
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        resolver
            .batch_execute("ROLLBACK")
            .await
            .expect("leave the failed resolution transaction");
        let state: tokio_postgres::Row = resolver
            .query_one(
                &format!(
                    "SELECT \
                       (SELECT count(*) FROM pg_catalog.pg_prepared_xacts WHERE gid = '{gid}'), \
                       (SELECT count(*) FROM compat_prepared_state_rows)"
                ),
                &[],
            )
            .await
            .expect("verify invalid-state resolution left the GID prepared and unpublished");
        let still_prepared: i64 = state.try_get(0).expect("prepared GID count");
        let visible_rows: i64 = state.try_get(1).expect("visible row count");
        let cleanup = if still_prepared == 1 {
            Some(
                owner
                    .batch_execute(&format!("ROLLBACK PREPARED '{gid}'"))
                    .await,
            )
        } else {
            None
        };

        // Assert
        drop((owner, resolver));
        server.shutdown(owner_connection).await;
        resolver_connection.abort();
        let _ = resolver_connection.await;
        assert_eq!(
            invalid_sqlstate.as_deref(),
            Some("25001"),
            "PostgreSQL rejects COMMIT PREPARED in a transaction block"
        );
        assert_eq!(still_prepared, 1);
        assert_eq!(visible_rows, 0);
        cleanup
            .expect("the GID remains prepared for cleanup")
            .expect("rollback the still-prepared transaction");
    });
}

#[test]
fn should_commit_prepared_transaction_once_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("prepared-transaction-commit-restart");
    let _directory = DataDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
        cassie.startup().expect("start Cassie instance");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE compat_915_restart_commit_rows (id INT PRIMARY KEY)",
            "BEGIN",
            "INSERT INTO compat_915_restart_commit_rows VALUES (18)",
            "PREPARE TRANSACTION 'compat_915_restart_commit_gid'",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("could not execute {sql}: {error}"));
        }
        drop(session);
        drop(cassie);
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    cassie
        .startup()
        .expect("recover prepared transaction state");
    let session = cassie.create_session("tester", None);
    let pending = cassie
        .execute_sql(
            &session,
            "SELECT count(*) FROM pg_catalog.pg_prepared_xacts \
             WHERE gid = 'compat_915_restart_commit_gid'",
            vec![],
        )
        .expect("read the prepared transaction after restart")
        .rows()
        .to_vec();
    let unpublished = cassie
        .execute_sql(
            &session,
            "SELECT count(*) FROM compat_915_restart_commit_rows",
            vec![],
        )
        .expect("confirm prepared changes remain unpublished after restart")
        .rows()
        .to_vec();
    cassie
        .execute_sql(
            &session,
            "COMMIT PREPARED 'compat_915_restart_commit_gid'",
            vec![],
        )
        .expect("commit the recovered prepared transaction");
    let committed_rows = cassie
        .execute_sql(
            &session,
            "SELECT id FROM compat_915_restart_commit_rows ORDER BY id",
            vec![],
        )
        .expect("read the committed row after resolving the recovered transaction")
        .rows()
        .to_vec();
    let resolved = cassie
        .execute_sql(
            &session,
            "SELECT count(*) FROM pg_catalog.pg_prepared_xacts \
             WHERE gid = 'compat_915_restart_commit_gid'",
            vec![],
        )
        .expect("verify the committed GID was removed")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(pending, vec![vec![cassie::types::Value::Int64(1)]]);
    assert_eq!(unpublished, vec![vec![cassie::types::Value::Int64(0)]]);
    assert_eq!(committed_rows, vec![vec![cassie::types::Value::Int64(18)]]);
    assert_eq!(resolved, vec![vec![cassie::types::Value::Int64(0)]]);
}

#[test]
fn should_rollback_prepared_transaction_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("prepared-transaction-rollback-restart");
    let _directory = DataDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
        cassie.startup().expect("start Cassie instance");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE compat_915_restart_rollback_rows (id INT PRIMARY KEY)",
            "BEGIN",
            "INSERT INTO compat_915_restart_rollback_rows VALUES (19)",
            "PREPARE TRANSACTION 'compat_915_restart_rollback_gid'",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("could not execute {sql}: {error}"));
        }
        drop(session);
        drop(cassie);
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    cassie
        .startup()
        .expect("recover prepared transaction state");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "ROLLBACK PREPARED 'compat_915_restart_rollback_gid'",
            vec![],
        )
        .expect("roll back the recovered prepared transaction");
    let rolled_back_rows = cassie
        .execute_sql(
            &session,
            "SELECT count(*) FROM compat_915_restart_rollback_rows",
            vec![],
        )
        .expect("confirm the prepared row was rolled back")
        .rows()
        .to_vec();
    let resolved = cassie
        .execute_sql(
            &session,
            "SELECT count(*) FROM pg_catalog.pg_prepared_xacts \
             WHERE gid = 'compat_915_restart_rollback_gid'",
            vec![],
        )
        .expect("verify the rolled-back GID was removed")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(rolled_back_rows, vec![vec![cassie::types::Value::Int64(0)]]);
    assert_eq!(resolved, vec![vec![cassie::types::Value::Int64(0)]]);
}
