#[test]
fn should_atomically_rollback_table_creation_after_insert() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_table_creation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the DDL transaction");

        // Act
        client
            .batch_execute("CREATE TABLE compat_transactional_ddl_rows (id INT PRIMARY KEY)")
            .await
            .expect("create a table inside the open transaction");
        client
            .batch_execute("INSERT INTO compat_transactional_ddl_rows VALUES (1)")
            .await
            .expect("insert data into the uncommitted table");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back both catalog and row changes");
        let table_count: i64 = client
            .query_one(
                "SELECT count(*) FROM information_schema.tables \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_transactional_ddl_rows'",
                &[],
            )
            .await
            .expect("inspect table publication after rollback")
            .try_get(0)
            .expect("matching table count");

        // Assert
        assert_eq!(table_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_rollback_alter_table_column_addition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_alter_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_rollback_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_alter_rollback_rows VALUES (1)",
                "BEGIN",
                "INSERT INTO compat_alter_rollback_rows VALUES (2)",
            ],
        )
        .await
        .expect("create the existing table and start a transaction");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_alter_rollback_rows \
                 ADD COLUMN label TEXT DEFAULT 'temporary'",
            )
            .await
            .expect("add a column inside the open transaction");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the catalog mutation");
        let column_count: i64 = client
            .query_one(
                "SELECT count(*) FROM information_schema.columns \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_alter_rollback_rows' \
                   AND column_name = 'label'",
                &[],
            )
            .await
            .expect("inspect the table after rollback")
            .try_get(0)
            .expect("matching column count");
        client
            .batch_execute("ALTER TABLE compat_alter_rollback_rows ADD COLUMN label TEXT")
            .await
            .expect("add the rolled-back column without a default");
        let rolled_back_value: Option<String> = client
            .query_one("SELECT label FROM compat_alter_rollback_rows", &[])
            .await
            .expect("read the row after re-adding the column")
            .try_get(0)
            .expect("nullable label value");
        let rows: i64 = client
            .query_one("SELECT count(*) FROM compat_alter_rollback_rows", &[])
            .await
            .expect("read the existing row after rollback")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(column_count, 0);
        assert_eq!(rolled_back_value, None);
        assert_eq!(rows, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_added_literal_default_to_rows_staged_before_alter_commit() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_alter_staged_default").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_alter_staged_default \
                 (id INT PRIMARY KEY, note TEXT)",
            )
            .await
            .expect("create the table before the transaction");
        client
            .batch_execute(
                "CREATE TABLE compat_alter_staged_other (id INT PRIMARY KEY)",
            )
            .await
            .expect("create a second table before the transaction");
        client
            .batch_execute("INSERT INTO compat_alter_staged_default VALUES (1, 'before')")
            .await
            .expect("seed a committed row");
        let initial_rows = client
            .query(
                "SELECT * FROM compat_alter_staged_default ORDER BY id",
                &[],
            )
            .await
            .expect("warm the plan cache before transactional DDL");
        assert_eq!(initial_rows[0].columns().len(), 2);
        execute_statements(
            &client,
            &[
                "BEGIN",
                "UPDATE compat_alter_staged_default SET note = 'updated' WHERE id = 1",
                "INSERT INTO compat_alter_staged_default VALUES (2, 'inserted')",
                "INSERT INTO compat_alter_staged_other VALUES (10)",
                "ALTER TABLE compat_alter_staged_default \
                 ADD COLUMN label TEXT DEFAULT 'added'",
                "INSERT INTO compat_alter_staged_default (id, note) VALUES (3, 'after')",
            ],
        )
        .await
        .expect("stage rows around the added default");

        // Act
        let staged_rows = client
            .query(
                "SELECT id, note, label FROM compat_alter_staged_default ORDER BY id",
                &[],
            )
            .await
            .expect("read the backfill and staged rows before commit");
        let staged_star_rows = client
            .query("SELECT * FROM compat_alter_staged_default ORDER BY id", &[])
            .await
            .expect("read the new column through the transaction catalog overlay");
        let staged_other_rows: i64 = client
            .query_one("SELECT count(*) FROM compat_alter_staged_other", &[])
            .await
            .expect("read the second table before commit")
            .try_get(0)
            .expect("staged other table row count");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit staged rows around the added default");

        let rows = client
            .query(
                "SELECT id, note, label FROM compat_alter_staged_default ORDER BY id",
                &[],
            )
            .await
            .expect("read the committed row");
        let other_rows: i64 = client
            .query_one("SELECT count(*) FROM compat_alter_staged_other", &[])
            .await
            .expect("read the row committed to the second table")
            .try_get(0)
            .expect("other table row count");

        // Assert
        assert_eq!(staged_rows.len(), 3);
        assert_eq!(staged_rows[0].get::<_, String>(2), "added");
        assert_eq!(staged_rows[1].get::<_, String>(2), "added");
        assert_eq!(staged_rows[2].get::<_, String>(2), "added");
        assert_eq!(staged_star_rows[0].columns().len(), 3);
        assert_eq!(staged_other_rows, 1);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "updated");
        assert_eq!(rows[0].get::<_, String>(2), "added");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "inserted");
        assert_eq!(rows[1].get::<_, String>(2), "added");
        assert_eq!(rows[2].get::<_, i32>(0), 3);
        assert_eq!(rows[2].get::<_, String>(1), "after");
        assert_eq!(rows[2].get::<_, String>(2), "added");
        assert_eq!(other_rows, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_existing_column_default_when_add_if_not_exists_runs_in_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_alter_existing_default").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_existing_default \
                 (id INT PRIMARY KEY, label TEXT DEFAULT 'original')",
                "INSERT INTO compat_alter_existing_default (id) VALUES (1)",
                "BEGIN",
                "INSERT INTO compat_alter_existing_default (id) VALUES (2)",
                "ALTER TABLE compat_alter_existing_default \
                 ADD COLUMN IF NOT EXISTS label TEXT DEFAULT 'replacement'",
                "COMMIT",
            ],
        )
        .await
        .expect("run repeated ADD COLUMN IF NOT EXISTS in a transaction");

        // Act
        let rows = client
            .query(
                "SELECT id, label FROM compat_alter_existing_default ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after commit");

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "original");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "original");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_commit_added_literal_default_backfill_without_other_staged_writes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_alter_overlay_only_commit").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_overlay_only (id INT PRIMARY KEY)",
                "INSERT INTO compat_alter_overlay_only VALUES (1)",
                "BEGIN",
                "ALTER TABLE compat_alter_overlay_only \
                 ADD COLUMN label TEXT DEFAULT 'committed'",
                "COMMIT",
            ],
        )
        .await
        .expect("commit DDL backfill without other transaction writes");

        // Act
        let value: String = client
            .query_one("SELECT label FROM compat_alter_overlay_only WHERE id = 1", &[])
            .await
            .expect("read the committed historical default")
            .try_get(0)
            .expect("literal default value");

        // Assert
        assert_eq!(value, "committed");

        drop(client);
        server.shutdown(connection).await;
    });
}
