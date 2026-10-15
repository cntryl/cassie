use super::*;

#[test]
fn should_report_cancellation_sqlstate_for_an_overdue_do_block() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_block_statement_timeout").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("SET statement_timeout = '25ms'")
            .await
            .expect("set a short timeout before running the anonymous block");

        // Act
        let cancelled = client
            .batch_execute("DO $$ BEGIN PERFORM pg_sleep(0.2); END $$")
            .await
            .expect_err("statement_timeout must cancel a sleeping DO block");
        let cancelled_sqlstate = db_error(&cancelled).code().code().to_string();
        client
            .batch_execute("RESET statement_timeout")
            .await
            .expect("reset the timeout after cancellation");
        let recovered: i32 = client
            .query_one("SELECT 1", &[])
            .await
            .expect("reuse the session after cancelling the DO statement")
            .get(0);

        // Assert
        assert_eq!(cancelled_sqlstate, "57014");
        assert_eq!(recovered, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
