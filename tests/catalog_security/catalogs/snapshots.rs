//! PostgreSQL transaction-snapshot visibility helpers (#817).

use crate::support_catalog_sessions as fixture;

#[test]
fn should_report_snapshot_bounds_given_two_active_transactions_when_snapshot_functions_are_called()
{
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let fixture = fixture::start("issue-817-snapshot-bounds").await;
        let first = fixture::connect(&fixture).await;
        let second = fixture::connect(&fixture).await;
        let observer = fixture::connect(&fixture).await;
        first
            .client
            .batch_execute("BEGIN")
            .await
            .expect("begin first active transaction");
        second
            .client
            .batch_execute("BEGIN")
            .await
            .expect("begin second active transaction");
        let first_xid: String = first
            .client
            .query_one("SELECT pg_current_xact_id()::text", &[])
            .await
            .expect("assign first transaction ID")
            .get(0);
        let second_xid: String = second
            .client
            .query_one("SELECT pg_current_xact_id()::text", &[])
            .await
            .expect("assign second transaction ID")
            .get(0);
        let sql = format!(
            "SELECT pg_catalog.pg_snapshot_xmin(snapshot) <= \
                    pg_catalog.pg_snapshot_xmax(snapshot), \
                    pg_catalog.pg_visible_in_snapshot('{first_xid}'::xid8, snapshot), \
                    pg_catalog.pg_visible_in_snapshot('{second_xid}'::xid8, snapshot) \
             FROM (SELECT pg_catalog.pg_current_snapshot() AS snapshot) AS captured"
        );

        // Act
        let row = observer
            .client
            .query_one(&sql, &[])
            .await
            .expect("inspect the active transaction IDs inside a snapshot");

        // Assert
        assert_eq!(
            (
                row.get::<_, bool>(0),
                row.get::<_, bool>(1),
                row.get::<_, bool>(2),
            ),
            (true, false, false)
        );
        fixture::stop(fixture, vec![first, second, observer]).await;
    });
}

#[test]
fn should_report_snapshot_visibility_given_committed_and_inflight_transactions_when_visibility_is_queried(
) {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let fixture = fixture::start("issue-817-snapshot-visibility").await;
        let inflight = fixture::connect(&fixture).await;
        let committed = fixture::connect(&fixture).await;
        let observer = fixture::connect(&fixture).await;
        inflight
            .client
            .batch_execute("BEGIN")
            .await
            .expect("begin the in-flight transaction");
        let inflight_xid: String = inflight
            .client
            .query_one("SELECT pg_current_xact_id()::text", &[])
            .await
            .expect("assign an in-flight transaction ID")
            .get(0);
        committed
            .client
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction to commit");
        let committed_xid: String = committed
            .client
            .query_one("SELECT pg_current_xact_id()::text", &[])
            .await
            .expect("assign a transaction ID to commit")
            .get(0);
        committed
            .client
            .batch_execute("COMMIT")
            .await
            .expect("commit the visibility control transaction");
        let sql = format!(
            "SELECT pg_catalog.pg_visible_in_snapshot(\
                        '{committed_xid}'::xid8, snapshot), \
                    pg_catalog.pg_visible_in_snapshot(\
                        '{inflight_xid}'::xid8, snapshot) \
             FROM (SELECT pg_catalog.pg_current_snapshot() AS snapshot) AS captured"
        );

        // Act
        let row = observer
            .client
            .query_one(&sql, &[])
            .await
            .expect("compare committed and active transactions with the snapshot");

        // Assert
        assert_eq!(
            (row.get::<_, bool>(0), row.get::<_, bool>(1)),
            (true, false)
        );
        fixture::stop(fixture, vec![inflight, committed, observer]).await;
    });
}

#[test]
fn should_preserve_legacy_txid_semantics_given_the_same_transaction_state_when_legacy_snapshot_helpers_are_called(
) {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let fixture = fixture::start("issue-817-legacy-snapshot-equivalence").await;
        let session = fixture::connect(&fixture).await;

        // Act
        let row = session
            .client
            .query_one(
                "SELECT txid_current_snapshot()::text = \
                        pg_catalog.pg_current_snapshot()::text",
                &[],
            )
            .await
            .expect("compare legacy and xid8 snapshot representations");

        // Assert
        assert!(row.get::<_, bool>(0));
        fixture::stop(fixture, vec![session]).await;
    });
}
