use super::*;

fn create_committed_column_cascade_state(path: &str) {
    let cassie = cassie::app::Cassie::new_with_data_dir_and_config(
        path,
        cassie::config::CassieRuntimeConfig::default(),
    )
    .expect("create local Cassie instance");
    cassie.startup().expect("start local Cassie instance");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE compat_839_restart_parent \
         (parent_id INT PRIMARY KEY, retained TEXT)",
        "CREATE TABLE compat_839_restart_child \
         (parent_id INT REFERENCES compat_839_restart_parent(parent_id))",
        "INSERT INTO compat_839_restart_parent VALUES (1, 'kept')",
        "INSERT INTO compat_839_restart_child VALUES (1)",
        "CREATE VIEW compat_839_restart_parent_view \
         AS SELECT parent_id FROM compat_839_restart_parent",
        "ALTER TABLE compat_839_restart_parent \
         DROP COLUMN parent_id CASCADE",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .unwrap_or_else(|error| panic!("execute setup statement {statement}: {error}"));
    }
}

#[test]
fn should_remove_dependent_view_when_dropping_column_with_cascade() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_drop_column_cascade_view").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE public.compat_839_drop_column \
                 (id INT, obsolete TEXT, retained INT)",
                "INSERT INTO public.compat_839_drop_column \
                 VALUES (1, 'remove', 42)",
                "CREATE VIEW public.compat_839_drop_column_view \
                 AS SELECT obsolete FROM public.compat_839_drop_column",
            ],
        )
        .await
        .expect("create a dependent view and seed columns that must survive the cascade");

        // Act
        let drop_column = client
            .batch_execute(
                "ALTER TABLE public.compat_839_drop_column \
                 DROP COLUMN obsolete CASCADE",
            )
            .await;
        let remaining_objects = if drop_column.is_ok() {
            let view_removed: bool = client
                .query_one(
                    "SELECT to_regclass(\
                       'public.compat_839_drop_column_view') IS NULL",
                    &[],
                )
                .await
                .expect("check that CASCADE removed the dependent view")
                .get(0);
            let columns = client
                .query(
                    "SELECT column_name FROM information_schema.columns \
                     WHERE table_schema = 'public' \
                       AND table_name = 'compat_839_drop_column' \
                     ORDER BY ordinal_position",
                    &[],
                )
                .await
                .expect("read columns that survive the cascading drop");
            let retained_row = client
                .query_one(
                    "SELECT id, retained FROM public.compat_839_drop_column",
                    &[],
                )
                .await
                .expect("read the row after dropping its dependent column");
            Some((view_removed, columns, retained_row))
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        drop_column.expect("DROP COLUMN CASCADE should remove its dependent view");
        let (view_removed, columns, retained_row) = remaining_objects
            .expect("inspect the relation, dependent view, and retained row after CASCADE");
        assert!(view_removed);
        assert_eq!(
            columns
                .iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>(),
            vec!["id".to_owned(), "retained".to_owned()]
        );
        assert_eq!(retained_row.get::<_, i32>(0), 1);
        assert_eq!(retained_row.get::<_, i32>(1), 42);
    });
}

#[test]
fn should_remove_foreign_key_when_dropping_referenced_column_with_cascade() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_drop_column_fk_cascade").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_839_fk_parent \
                 (parent_id INT PRIMARY KEY, retained TEXT)",
                "CREATE TABLE compat_839_fk_child \
                 (parent_id INT REFERENCES compat_839_fk_parent(parent_id))",
                "INSERT INTO compat_839_fk_parent VALUES (1, 'kept')",
                "INSERT INTO compat_839_fk_child VALUES (1)",
                "CREATE VIEW compat_839_fk_parent_view \
                 AS SELECT parent_id FROM compat_839_fk_parent",
            ],
        )
        .await
        .expect("create a referenced key, foreign key and dependent view");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_839_fk_parent \
                 DROP COLUMN parent_id CASCADE",
            )
            .await
            .expect("drop the referenced key and all dependent objects with CASCADE");
        let foreign_key_insert = client
            .execute("INSERT INTO compat_839_fk_child VALUES (99)", &[])
            .await;
        let retained_value: String = client
            .query_one("SELECT retained FROM compat_839_fk_parent", &[])
            .await
            .expect("read the surviving parent column")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            foreign_key_insert.is_ok(),
            "CASCADE should remove the foreign key constraint: {foreign_key_insert:?}"
        );
        assert_eq!(retained_value, "kept");
    });
}

async fn assert_cascaded_view_is_hidden(client: &tokio_postgres::Client) {
    let view_count = client
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_class \
             WHERE relname = 'compat_839_rollback_parent_view'",
            &[],
        )
        .await
        .expect("inspect the view after cascading its source column")
        .get::<_, i64>(0);

    assert_eq!(view_count, 0);
}

async fn assert_column_cascade_dependencies_are_restored(client: &tokio_postgres::Client) {
    let parent_columns = client
        .query(
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name = 'compat_839_rollback_parent' \
             ORDER BY ordinal_position",
            &[],
        )
        .await
        .expect("read restored parent columns");
    assert_eq!(
        parent_columns
            .iter()
            .map(|row| row.get::<_, String>(0))
            .collect::<Vec<_>>(),
        vec!["parent_id".to_owned(), "retained".to_owned()]
    );

    let view_rows = client
        .query(
            "SELECT parent_id FROM compat_839_rollback_parent_view ORDER BY parent_id",
            &[],
        )
        .await
        .expect("read the restored dependent view");
    assert_eq!(
        view_rows
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>(),
        vec![1]
    );

    let foreign_key_count = client
        .query_one(
            "SELECT count(*) FROM pg_catalog.pg_constraint \
             WHERE conrelid = 'compat_839_rollback_child' \
               AND contype = 'f'",
            &[],
        )
        .await
        .expect("inspect the restored foreign key")
        .get::<_, i64>(0);
    assert_eq!(foreign_key_count, 1);

    let orphan_insert = client
        .execute("INSERT INTO compat_839_rollback_child VALUES (99)", &[])
        .await
        .expect_err("the restored foreign key should reject an orphan");
    assert_eq!(
        orphan_insert
            .as_db_error()
            .expect("foreign-key violation details")
            .code()
            .code(),
        "23503"
    );
}

#[test]
fn should_restore_dropped_column_dependencies_after_transaction_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_drop_column_cascade_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_839_rollback_parent \
                 (parent_id INT PRIMARY KEY, retained TEXT)",
                "CREATE TABLE compat_839_rollback_child \
                 (parent_id INT REFERENCES compat_839_rollback_parent(parent_id))",
                "INSERT INTO compat_839_rollback_parent VALUES (1, 'kept')",
                "INSERT INTO compat_839_rollback_child VALUES (1)",
                "CREATE VIEW compat_839_rollback_parent_view \
                 AS SELECT parent_id FROM compat_839_rollback_parent",
                "BEGIN",
            ],
        )
        .await
        .expect("create the dependent objects and begin the transaction");

        // Act
        let drop_column = client
            .batch_execute(
                "ALTER TABLE compat_839_rollback_parent \
                 DROP COLUMN parent_id CASCADE",
            )
            .await;
        drop_column.expect("DROP COLUMN CASCADE should stage successfully");
        assert_cascaded_view_is_hidden(&client).await;
        let rollback = client.batch_execute("ROLLBACK").await;

        // Assert
        rollback.expect("roll back the column and dependent-object removal");
        assert_column_cascade_dependencies_are_restored(&client).await;
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_recover_column_cascade_dependency_removal_after_local_restart() {
    // Arrange
    let path = support::data_dir("issue_839_column_cascade_restart");
    let _directory = DataDirectory(path.clone());
    create_committed_column_cascade_state(&path);

    // Act
    let cassie = cassie::app::Cassie::new_with_data_dir_and_config(
        &path,
        cassie::config::CassieRuntimeConfig::default(),
    )
    .expect("reopen local Cassie instance");
    cassie.startup().expect("recover committed column cascade");
    let session = cassie.create_session("tester", None);
    let parent_columns = cassie
        .execute_sql(
            &session,
            "SELECT column_name FROM information_schema.columns \
             WHERE table_name = 'compat_839_restart_parent' \
             ORDER BY ordinal_position",
            vec![],
        )
        .expect("read parent columns after restart")
        .rows()
        .to_vec();
    let view_count = cassie
        .execute_sql(
            &session,
            "SELECT count(*) FROM pg_catalog.pg_class \
             WHERE relname = 'compat_839_restart_parent_view'",
            vec![],
        )
        .expect("inspect the cascaded view after restart")
        .rows()[0][0]
        .clone();
    let foreign_key_count = cassie
        .execute_sql(
            &session,
            "SELECT count(*) FROM pg_catalog.pg_constraint \
             WHERE conrelid = 'compat_839_restart_child' \
               AND contype = 'f'",
            vec![],
        )
        .expect("inspect the cascaded foreign key after restart")
        .rows()[0][0]
        .clone();
    let retained_rows = cassie
        .execute_sql(
            &session,
            "SELECT retained FROM compat_839_restart_parent",
            vec![],
        )
        .expect("read surviving parent data after restart")
        .rows()
        .to_vec();
    let orphan_insert = cassie.execute_sql(
        &session,
        "INSERT INTO compat_839_restart_child VALUES (99)",
        vec![],
    );

    // Assert
    assert_eq!(
        parent_columns,
        vec![vec![cassie::types::Value::String("retained".to_owned())]]
    );
    assert_eq!(view_count, cassie::types::Value::Int64(0));
    assert_eq!(foreign_key_count, cassie::types::Value::Int64(0));
    assert_eq!(
        retained_rows,
        vec![vec![cassie::types::Value::String("kept".to_owned())]]
    );
    orphan_insert.expect("the dropped referenced column removes its foreign key");
}
