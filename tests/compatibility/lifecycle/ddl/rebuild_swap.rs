//! Pgwire visibility of the selected staging-table rebuild swap.

use super::*;

async fn table_row(client: &tokio_postgres::Client, sql: &str) -> (i64, String) {
    let row = client
        .query_one(sql, &[])
        .await
        .expect("read rebuild table");
    (row.get(0), row.get(1))
}

#[test]
fn should_publish_rebuild_swap_to_other_pgwire_sessions_only_after_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_826_rebuild_swap_wire").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE wire_rebuild_live (id BIGINT, label TEXT)",
                "CREATE TABLE wire_rebuild_stage (id BIGINT, label TEXT)",
                "INSERT INTO wire_rebuild_live VALUES (1, 'old')",
                "INSERT INTO wire_rebuild_stage VALUES (2, 'new')",
            ],
        )
        .await
        .expect("create and load separate live and staging tables");

        // Act
        execute_statements(
            &owner,
            &[
                "BEGIN",
                "ALTER TABLE wire_rebuild_live RENAME TO wire_rebuild_old",
                "ALTER TABLE wire_rebuild_stage RENAME TO wire_rebuild_live",
            ],
        )
        .await
        .expect("stage both renames as separate protocol statements");
        let owner_live = table_row(&owner, "SELECT id, label FROM wire_rebuild_live").await;
        let owner_old = table_row(&owner, "SELECT id, label FROM wire_rebuild_old").await;
        let observer_live = table_row(&observer, "SELECT id, label FROM wire_rebuild_live").await;
        let observer_stage = table_row(&observer, "SELECT id, label FROM wire_rebuild_stage").await;
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back swap");
        let rollback_live = table_row(&owner, "SELECT id, label FROM wire_rebuild_live").await;
        let rollback_stage = table_row(&owner, "SELECT id, label FROM wire_rebuild_stage").await;
        execute_statements(
            &owner,
            &[
                "BEGIN",
                "ALTER TABLE wire_rebuild_live RENAME TO wire_rebuild_old",
                "ALTER TABLE wire_rebuild_stage RENAME TO wire_rebuild_live",
                "COMMIT",
            ],
        )
        .await
        .expect("retry and commit both renames");
        let committed_live = table_row(&observer, "SELECT id, label FROM wire_rebuild_live").await;
        let committed_old = table_row(&observer, "SELECT id, label FROM wire_rebuild_old").await;
        let stage_count: i64 = observer
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'wire_rebuild_stage'",
                &[],
            )
            .await
            .expect("check the former staging name after commit")
            .get(0);

        // Assert
        let old = (1, "old".to_owned());
        let new = (2, "new".to_owned());
        assert_eq!(owner_live, new);
        assert_eq!(owner_old, old);
        assert_eq!(observer_live, old);
        assert_eq!(observer_stage, new);
        assert_eq!(rollback_live, old);
        assert_eq!(rollback_stage, new);
        assert_eq!(committed_live, new);
        assert_eq!(committed_old, old);
        assert_eq!(stage_count, 0);

        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}
