use super::*;

#[test]
fn should_retain_row_lock_until_prepared_transaction_resolution() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_transaction_lock_lifetime").await;
        let (preparer, preparer_connection) = server.connect().await;
        let (resolver, resolver_connection) = server.connect().await;
        execute_statements(
            &preparer,
            &[
                "CREATE TABLE compat_prepared_lock_lifetime (id INT PRIMARY KEY)",
                "INSERT INTO compat_prepared_lock_lifetime VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the row and begin its prepared transaction");
        let lock = preparer
            .query_one(
                "SELECT id FROM compat_prepared_lock_lifetime WHERE id = 1 FOR UPDATE",
                &[],
            )
            .await;
        let gid = "compat_prepared_lock_lifetime_gid";

        // Act
        let prepare = if lock.is_ok() {
            Some(
                preparer
                    .batch_execute(&format!("PREPARE TRANSACTION '{gid}'"))
                    .await,
            )
        } else {
            None
        };
        let observations = if prepare.as_ref().is_some_and(|result| result.is_ok()) {
            let catalog = resolver
                .query_one(
                    &format!(
                        "SELECT gid, prepared IS NOT NULL \
                         FROM pg_catalog.pg_prepared_xacts WHERE gid = '{gid}'"
                    ),
                    &[],
                )
                .await;
            let blocked = resolver
                .query_one(
                    "SELECT id FROM compat_prepared_lock_lifetime \
                     WHERE id = 1 FOR UPDATE NOWAIT",
                    &[],
                )
                .await;
            let resolution = resolver
                .batch_execute(&format!("ROLLBACK PREPARED '{gid}'"))
                .await;
            let released = resolver
                .query_one(
                    "SELECT id FROM compat_prepared_lock_lifetime \
                     WHERE id = 1 FOR UPDATE NOWAIT",
                    &[],
                )
                .await;
            Some((catalog, blocked, resolution, released))
        } else {
            let _ = preparer.batch_execute("ROLLBACK").await;
            None
        };

        drop((preparer, resolver));
        server.shutdown(preparer_connection).await;
        resolver_connection.abort();
        let _ = resolver_connection.await;

        // Assert
        assert!(
            lock.is_ok(),
            "PostgreSQL accepts SELECT FOR UPDATE before PREPARE TRANSACTION: {lock:?}"
        );
        prepare
            .expect("a locked row must reach PREPARE TRANSACTION")
            .expect("PREPARE TRANSACTION should preserve the row lock");
        let (catalog, blocked, resolution, released) =
            observations.expect("prepared transaction lock observations");
        let catalog = catalog.expect("inspect the prepared transaction from another session");
        assert_eq!(catalog.get::<_, String>(0), gid);
        assert!(catalog.get::<_, bool>(1));
        let blocked = blocked.expect_err("the prepared transaction must retain its row lock");
        assert_eq!(db_error(&blocked).code().code(), "55P03");
        resolution.expect("ROLLBACK PREPARED releases the retained row lock");
        let released = released.expect("acquire the row lock after prepared resolution");
        assert_eq!(released.get::<_, i32>(0), 1);
    });
}

#[test]
fn should_retain_table_lock_until_prepared_transaction_resolution() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_transaction_table_lock_lifetime").await;
        let (preparer, preparer_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        let (resolver, resolver_connection) = server.connect().await;
        preparer
            .batch_execute("CREATE TABLE compat_prepared_table_lock (id INT)")
            .await
            .expect("create the relation before preparing its table lock");
        preparer
            .batch_execute("BEGIN; LOCK TABLE compat_prepared_table_lock IN ACCESS EXCLUSIVE MODE")
            .await
            .expect("acquire an exclusive table lock in the transaction");
        let gid = "compat_prepared_table_lock_gid";

        // Act
        let prepare = preparer
            .batch_execute(&format!("PREPARE TRANSACTION '{gid}'"))
            .await;
        let observations = if prepare.is_ok() {
            contender
                .batch_execute("BEGIN")
                .await
                .expect("begin the competing lock transaction");
            let blocked = contender
                .batch_execute(
                    "LOCK TABLE compat_prepared_table_lock \
                     IN ACCESS SHARE MODE NOWAIT",
                )
                .await;
            contender
                .batch_execute("ROLLBACK")
                .await
                .expect("end the contender transaction after its lock attempt");
            let resolution = resolver
                .batch_execute(&format!("ROLLBACK PREPARED '{gid}'"))
                .await;
            resolver
                .batch_execute("BEGIN")
                .await
                .expect("begin the post-resolution lock check");
            let released = resolver
                .batch_execute(
                    "LOCK TABLE compat_prepared_table_lock \
                     IN ACCESS SHARE MODE NOWAIT",
                )
                .await;
            resolver
                .batch_execute("ROLLBACK")
                .await
                .expect("release the post-resolution table lock");
            Some((blocked, resolution, released))
        } else {
            let _ = preparer.batch_execute("ROLLBACK").await;
            None
        };

        drop((preparer, contender, resolver));
        server.shutdown(preparer_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
        resolver_connection.abort();
        let _ = resolver_connection.await;

        // Assert
        prepare.expect("PostgreSQL prepares a transaction holding a table lock");
        let (blocked, resolution, released) =
            observations.expect("prepared table-lock observations");
        let blocked = blocked.expect_err("the prepared ACCESS EXCLUSIVE lock blocks ACCESS SHARE");
        assert_eq!(db_error(&blocked).code().code(), "55P03");
        resolution.expect("ROLLBACK PREPARED releases the retained table lock");
        released.expect("ACCESS SHARE succeeds after prepared resolution");
    });
}
