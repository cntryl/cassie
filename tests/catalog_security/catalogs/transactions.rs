//! PostgreSQL transaction-status catalog helper (#817).

use crate::support_catalog_sessions as fixture;

#[test]
fn should_report_transaction_status_given_assigned_transaction_ids_when_transaction_outcomes_are_queried(
) {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let fixture = fixture::start("issue-817-transaction-status").await;
        let active = fixture::connect(&fixture).await;
        let committed = fixture::connect(&fixture).await;
        let aborted = fixture::connect(&fixture).await;
        let observer = fixture::connect(&fixture).await;
        let mut transaction_ids = Vec::with_capacity(3);
        for session in [&active, &committed, &aborted] {
            session
                .client
                .batch_execute("BEGIN")
                .await
                .expect("begin a transaction with an assigned ID");
            transaction_ids.push(
                session
                    .client
                    .query_one("SELECT pg_current_xact_id()::text", &[])
                    .await
                    .expect("assign a transaction ID")
                    .get::<_, String>(0),
            );
        }
        committed
            .client
            .batch_execute("COMMIT")
            .await
            .expect("commit the second transaction");
        aborted
            .client
            .batch_execute("ROLLBACK")
            .await
            .expect("abort the third transaction");
        let sql = format!(
            "SELECT pg_catalog.pg_xact_status('{0}'::xid8), \
                    pg_catalog.pg_xact_status('{1}'::xid8), \
                    pg_catalog.pg_xact_status('{2}'::xid8)",
            transaction_ids[0], transaction_ids[1], transaction_ids[2]
        );

        // Act
        let row = observer
            .client
            .query_one(&sql, &[])
            .await
            .expect("read active, committed, and aborted transaction status");

        // Assert
        assert_eq!(
            vec![
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                row.get::<_, String>(2),
            ],
            vec![
                "in progress".to_owned(),
                "committed".to_owned(),
                "aborted".to_owned(),
            ]
        );
        fixture::stop(fixture, vec![active, committed, aborted, observer]).await;
    });
}
