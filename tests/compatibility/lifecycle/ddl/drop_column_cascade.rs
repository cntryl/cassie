use super::*;

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
