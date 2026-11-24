//! Indexed expression reads must agree with a scan after transaction rollback.

use super::*;

async fn expression_rows(client: &tokio_postgres::Client, table: &str, key: &str) -> Vec<i64> {
    let sql = format!("SELECT id FROM {table} WHERE lower(email) = '{key}' ORDER BY id");
    client
        .query(&sql, &[])
        .await
        .expect("read expression-key matches")
        .iter()
        .map(|row| row.get(0))
        .collect()
}

async fn expression_index_plan(client: &tokio_postgres::Client) -> String {
    client
        .query_one(
            "EXPLAIN SELECT id FROM expression_index_rollback_indexed \
             WHERE lower(email) = 'alice@new.example.com' ORDER BY id",
            &[],
        )
        .await
        .expect("explain the expression-index query")
        .get(0)
}

async fn stage_mutations(client: &tokio_postgres::Client) {
    for table in [
        "expression_index_rollback_indexed",
        "expression_index_rollback_scan",
    ] {
        for sql in [
            format!("UPDATE {table} SET email = 'ALICE@NEW.EXAMPLE.COM' WHERE id = 1"),
            format!("DELETE FROM {table} WHERE id = 2"),
            format!("INSERT INTO {table} VALUES (4, 'alice@new.example.com')"),
        ] {
            client.batch_execute(&sql).await.expect("stage mutation");
        }
    }
}

async fn check_index_and_scan(
    client: &tokio_postgres::Client,
    old_expected: &[i64],
    new_expected: &[i64],
    deleted_expected: &[i64],
) {
    for (key, expected) in [
        ("alice@example.com", old_expected),
        ("alice@new.example.com", new_expected),
        ("bob@example.com", deleted_expected),
    ] {
        let indexed = expression_rows(client, "expression_index_rollback_indexed", key).await;
        let scanned = expression_rows(client, "expression_index_rollback_scan", key).await;
        assert_eq!(indexed, expected, "unexpected indexed rows for {key}");
        assert_eq!(
            scanned, expected,
            "unexpected independent scan rows for {key}"
        );
    }
}

async fn check_plans(client: &tokio_postgres::Client) {
    let plan = expression_index_plan(client).await;
    assert!(
        plan.contains("expression_index_rollback_lower_idx"),
        "expected the expression index to serve the new-key query: {plan}"
    );
    let scan_plan: String = client
        .query_one(
            "EXPLAIN SELECT id FROM expression_index_rollback_scan \
             WHERE lower(email) = 'alice@new.example.com' ORDER BY id",
            &[],
        )
        .await
        .expect("explain the unindexed reference query")
        .get(0);
    assert!(
        scan_plan.contains("index_aware=false index=none"),
        "the reference query must use an unindexed scan: {scan_plan}"
    );
}

#[test]
fn should_preserve_expression_index_scan_equivalence_through_transaction_resolution() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("expression_index_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE expression_index_rollback_indexed (id BIGINT, email TEXT NOT NULL)",
                "CREATE TABLE expression_index_rollback_scan (id BIGINT, email TEXT NOT NULL)",
                "INSERT INTO expression_index_rollback_indexed VALUES \
                   (1, 'Alice@example.com'), (2, 'bob@example.com'), (3, 'carol@example.com')",
                "INSERT INTO expression_index_rollback_scan VALUES \
                   (1, 'Alice@example.com'), (2, 'bob@example.com'), (3, 'carol@example.com')",
                "CREATE INDEX expression_index_rollback_lower_idx \
                   ON expression_index_rollback_indexed ((lower(email)))",
            ],
        )
        .await
        .expect("create populated indexed and independent scan tables");
        check_index_and_scan(&client, &[1], &[], &[2]).await;
        check_plans(&client).await;

        // Act
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin transaction");
        stage_mutations(&client).await;
        check_index_and_scan(&client, &[], &[1, 4], &[]).await;
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back mutations");

        // Assert
        check_index_and_scan(&client, &[1], &[], &[2]).await;
        check_plans(&client).await;

        client
            .batch_execute("BEGIN")
            .await
            .expect("begin committed transaction");
        stage_mutations(&client).await;
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit mutations");
        check_index_and_scan(&client, &[], &[1, 4], &[]).await;
        check_plans(&client).await;

        drop(client);
        server.shutdown(connection).await;
    });
}
