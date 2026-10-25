use super::*;

#[path = "enum_catalog_snapshot_visibility.rs"]
mod postgresql18_enum_catalog_snapshot_visibility;
#[path = "enum_identity_lifecycle.rs"]
mod postgresql18_enum_identity_lifecycle;
#[path = "enum_record_domain.rs"]
mod postgresql18_enum_record_domain_wave6;
#[path = "enum_support_function_identity.rs"]
mod postgresql18_enum_support_function_identity;

#[test]
fn should_order_enum_first_last_range_by_label() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_support_functions").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TYPE compat_priority AS ENUM ('low', 'medium', 'high')")
            .await
            .expect("create an enum with ordered labels");
        execute_statements(
            &client,
            &[
                "ALTER TYPE compat_priority ADD VALUE IF NOT EXISTS 'medium_high' AFTER 'medium'",
                "ALTER TYPE compat_priority ADD VALUE IF NOT EXISTS 'medium_high' AFTER 'medium'",
            ],
        )
        .await
        .expect("add an enum label after its neighbor idempotently");

        // Act
        let row = client
            .query_one(
                "SELECT enum_first(NULL::compat_priority)::TEXT, \
                 enum_last(NULL::compat_priority)::TEXT, \
                 enum_range(NULL::compat_priority)::TEXT, \
                 enum_range('low'::compat_priority, 'medium'::compat_priority)::TEXT",
                &[],
            )
            .await
            .expect("read enum boundary and range functions");

        // Assert
        assert_eq!(row.get::<_, String>(0), "low");
        assert_eq!(row.get::<_, String>(1), "high");
        assert_eq!(row.get::<_, String>(2), "{low,medium,medium_high,high}");
        assert_eq!(row.get::<_, String>(3), "{low,medium}");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_update_existing_rows_after_enum_label_rename() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_value_rename_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_mood_rename AS ENUM ('sad', 'okay', 'happy')",
                "CREATE TABLE compat_mood_rows (id INT PRIMARY KEY, mood compat_mood_rename)",
                "INSERT INTO compat_mood_rows VALUES (1, 'okay'), (2, 'happy')",
            ],
        )
        .await
        .expect("create an enum and rows that use its labels");

        // Act
        client
            .batch_execute("ALTER TYPE compat_mood_rename RENAME VALUE 'okay' TO 'calm'")
            .await
            .expect("rename an enum label without replacing its stored values");
        let rows = client
            .query(
                "SELECT id, mood::TEXT FROM compat_mood_rows ORDER BY id",
                &[],
            )
            .await
            .expect("read rows stored before the enum label rename");
        let labels: String = client
            .query_one("SELECT enum_range(NULL::compat_mood_rename)::TEXT", &[])
            .await
            .expect("read the enum labels in sort order")
            .get(0);

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "calm");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "happy");
        assert_eq!(labels, "{sad,calm,happy}");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_require_enum_value_addition_to_commit_before_use() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_value_commit_before_use").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_tx_mood AS ENUM ('waiting')",
                "CREATE TABLE compat_tx_mood_rows (mood compat_tx_mood)",
            ],
        )
        .await
        .expect("create an enum and table for transactional label use");

        // Act
        client
            .batch_execute("BEGIN; ALTER TYPE compat_tx_mood ADD VALUE 'ready'")
            .await
            .expect("add a new enum label inside a transaction");
        let unsafe_use = client
            .execute("INSERT INTO compat_tx_mood_rows VALUES ('ready')", &[])
            .await
            .expect_err("reject use of an enum value before its transaction commits");
        let unsafe_use_sqlstate = db_error(&unsafe_use).code().code().to_owned();
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the failed enum-label transaction");
        let labels_after_rollback: String = client
            .query_one("SELECT enum_range(NULL::compat_tx_mood)::TEXT", &[])
            .await
            .expect("read enum labels after rolling back their addition")
            .get(0);
        client
            .batch_execute("ALTER TYPE compat_tx_mood ADD VALUE 'ready'")
            .await
            .expect("commit the label addition in its own statement");
        client
            .execute("INSERT INTO compat_tx_mood_rows VALUES ('ready')", &[])
            .await
            .expect("use the label after the addition committed");
        let stored: String = client
            .query_one("SELECT mood::TEXT FROM compat_tx_mood_rows", &[])
            .await
            .expect("read the enum value after commit")
            .get(0);

        // Assert
        assert_eq!(unsafe_use_sqlstate, "55P04");
        assert_eq!(labels_after_rollback, "{waiting}");
        assert_eq!(stored, "ready");

        drop(client);
        server.shutdown(connection).await;
    });
}
