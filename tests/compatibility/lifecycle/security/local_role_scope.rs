use super::*;

async fn read_current_session_identity(client: &tokio_postgres::Client) -> (String, String) {
    let row = client
        .query_one("SELECT current_user, session_user", &[])
        .await
        .expect("read the current and session identities");
    (row.get::<_, String>(0), row.get::<_, String>(1))
}

#[test]
fn should_restore_local_role_at_transaction_boundaries() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("local_role_transaction_scope").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE ROLE compat_local_role_target NOLOGIN")
            .await
            .expect("create the role used for the transaction-local switch");
        let original_identity = read_current_session_identity(&client).await;

        // Act
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin commit case");
        client
            .batch_execute("SET LOCAL ROLE compat_local_role_target")
            .await
            .expect("set a role only for the current transaction");
        let commit_case_identity = read_current_session_identity(&client).await;
        client
            .batch_execute("SAVEPOINT before_missing_role")
            .await
            .expect("save the active role before probing an unknown target");
        let missing_role_error = client
            .batch_execute("SET LOCAL ROLE compat_local_role_missing")
            .await
            .expect_err("reject a transaction-local switch to a missing role");
        client
            .batch_execute("ROLLBACK TO SAVEPOINT before_missing_role")
            .await
            .expect("restore the active role after the rejected switch");
        let after_error_identity = read_current_session_identity(&client).await;
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit the transaction-local role scope");
        let after_commit_identity = read_current_session_identity(&client).await;

        client
            .batch_execute("BEGIN")
            .await
            .expect("begin rollback case");
        client
            .batch_execute("SET LOCAL ROLE compat_local_role_target")
            .await
            .expect("set the transaction-local role before rollback");
        let rollback_case_identity = read_current_session_identity(&client).await;
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction-local role scope");
        let after_rollback_identity = read_current_session_identity(&client).await;

        // Assert
        assert_eq!(
            commit_case_identity,
            (
                "compat_local_role_target".to_owned(),
                original_identity.1.clone(),
            )
        );
        assert_eq!(db_error(&missing_role_error).code().code(), "22023");
        assert_eq!(
            after_error_identity,
            (
                "compat_local_role_target".to_owned(),
                original_identity.1.clone(),
            ),
            "the invalid role target must not replace the active local role",
        );
        assert_eq!(after_commit_identity, original_identity);
        assert_eq!(
            rollback_case_identity,
            (
                "compat_local_role_target".to_owned(),
                original_identity.1.clone(),
            )
        );
        assert_eq!(after_rollback_identity, original_identity);

        client
            .batch_execute("DROP ROLE compat_local_role_target")
            .await
            .expect("drop the temporary role after restoring the session identity");
        drop(client);
        server.shutdown(connection).await;
    });
}
