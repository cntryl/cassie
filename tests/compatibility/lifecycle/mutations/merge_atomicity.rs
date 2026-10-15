use super::*;

#[test]
fn should_rollback_prior_merge_actions_given_a_later_constraint_violation_when_merge_executes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("merge_late_constraint_atomicity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_merge_atomic_target ( \
                   id INT PRIMARY KEY, amount INT NOT NULL CHECK (amount < 10))",
            )
            .await
            .expect("create the target with a row-level check constraint");
        client
            .batch_execute("INSERT INTO compat_merge_atomic_target VALUES (1, 1)")
            .await
            .expect("seed the row that the first MERGE action will update");

        // Act
        let merge = client
            .execute(
                "MERGE INTO compat_merge_atomic_target AS target \
                 USING (VALUES (1, 2), (2, 99)) AS source(id, amount) \
                   ON target.id = source.id \
                 WHEN MATCHED THEN UPDATE SET amount = source.amount \
                 WHEN NOT MATCHED THEN INSERT (id, amount) \
                   VALUES (source.id, source.amount)",
                &[],
            )
            .await;
        let rows = client
            .query(
                "SELECT id, amount FROM compat_merge_atomic_target ORDER BY id",
                &[],
            )
            .await
            .expect("inspect target contents after the rejected MERGE");

        // Assert
        assert_eq!(
            db_error(
                merge
                    .as_ref()
                    .expect_err("the later source row violates CHECK")
            )
            .code()
            .code(),
            "23514"
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, i32>(1), 1);
        drop(client);
        server.shutdown(connection).await;
    });
}
