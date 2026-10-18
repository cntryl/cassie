use super::*;

#[test]
fn should_check_deferred_partial_exclusion_when_constraint_becomes_immediate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("deferred_partial_exclusion_immediate").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_deferred_partial_exclusion (\
                   id INT PRIMARY KEY, slot INT NOT NULL, active BOOL NOT NULL, \
                   CONSTRAINT compat_deferred_partial_exclusion_key \
                     EXCLUDE USING btree (slot WITH =) WHERE (active) \
                     DEFERRABLE INITIALLY DEFERRED)",
                "INSERT INTO compat_deferred_partial_exclusion VALUES \
                   (1, 4, false), (2, 4, false)",
            ],
        )
        .await
        .expect("create a deferred partial exclusion and commit duplicate excluded rows");
        let metadata = client
            .query_one(
                "SELECT contype::TEXT, condeferrable, condeferred \
                 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_deferred_partial_exclusion_key'",
                &[],
            )
            .await
            .expect("inspect the partial exclusion constraint mode");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for active rows covered by the predicate");
        let first_active = client
            .execute(
                "INSERT INTO compat_deferred_partial_exclusion VALUES (3, 9, true)",
                &[],
            )
            .await
            .expect("defer the first active key check");
        let second_active = client
            .execute(
                "INSERT INTO compat_deferred_partial_exclusion VALUES (4, 9, true)",
                &[],
            )
            .await
            .expect("defer the duplicate active key check until constraint mode changes");

        // Act
        let immediate = client
            .batch_execute("SET CONSTRAINTS compat_deferred_partial_exclusion_key IMMEDIATE")
            .await
            .expect_err("changing the exclusion constraint to IMMEDIATE must check queued rows");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction after the deferred conflict is detected");
        let rows = client
            .query(
                "SELECT id, slot, active FROM compat_deferred_partial_exclusion ORDER BY id",
                &[],
            )
            .await
            .expect("read previously committed rows after rolling back the active conflict");

        // Assert
        assert_eq!(metadata.get::<_, String>(0), "x");
        assert!(metadata.get::<_, bool>(1));
        assert!(metadata.get::<_, bool>(2));
        assert_eq!(first_active, 1);
        assert_eq!(second_active, 1);
        assert_eq!(db_error(&immediate).code().code(), "23P01");
        assert_eq!(
            rows.len(),
            2,
            "rollback must remove both pending active rows"
        );
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, i32>(1), 4);
        assert!(!rows[0].get::<_, bool>(2));
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, i32>(1), 4);
        assert!(!rows[1].get::<_, bool>(2));

        drop(client);
        server.shutdown(connection).await;
    });
}
