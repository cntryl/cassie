use super::*;

// This #835 acceptance scenario checks transaction boundaries inside a
// procedure invoked by a top-level CALL. PostgreSQL 18.6 commits the first
// insert, then rolls back the second. On the current Cassie baseline, CREATE
// PROCEDURE LANGUAGE plpgsql may fail before CALL; that declaration failure
// is a prerequisite gap and is not evidence about transaction control itself.
#[test]
fn should_preserve_pre_rollback_rows_after_procedure_transaction_control() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("procedure_transaction_control").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_procedure_transaction_rows (marker TEXT PRIMARY KEY)",
            )
            .await
            .expect("create the table used to observe procedure transaction boundaries");
        let procedure_creation = client
            .batch_execute(
                "CREATE PROCEDURE compat_procedure_commit_then_rollback() \
                   LANGUAGE plpgsql AS $procedure$ \
                   BEGIN \
                     INSERT INTO compat_procedure_transaction_rows VALUES ('committed'); \
                     COMMIT; \
                     INSERT INTO compat_procedure_transaction_rows VALUES ('rolled-back'); \
                     ROLLBACK; \
                   END; \
                   $procedure$",
            )
            .await;
        procedure_creation.unwrap_or_else(|error| {
            panic!(
                "PostgreSQL 18.6 accepts this PL/pgSQL procedure declaration; Cassie rejected \
                 the procedure body before CALL, exposing the #835 procedure-language \
                 prerequisite and providing no evidence about CALL transaction control: {error}"
            )
        });

        // Act
        client
            .batch_execute("CALL compat_procedure_commit_then_rollback()")
            .await
            .expect("call the procedure at top level so transaction control is allowed");
        let markers = client
            .query(
                "SELECT marker FROM compat_procedure_transaction_rows ORDER BY marker",
                &[],
            )
            .await
            .expect("read rows after the procedure commits and rolls back");

        // Assert
        assert_eq!(
            markers
                .iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>(),
            vec!["committed".to_owned()]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
