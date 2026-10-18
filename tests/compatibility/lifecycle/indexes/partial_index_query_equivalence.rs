use super::*;

async fn query_partial_index_rows(
    client: &tokio_postgres::Client,
    predicate: &str,
    ordering: &str,
) -> Vec<(i32, Option<i32>, bool, String)> {
    let sql = format!(
        "SELECT id, key_value, active, payload \
         FROM compat_partial_index_equivalence_rows \
         WHERE {predicate} ORDER BY {ordering}"
    );
    client
        .query(&sql, &[])
        .await
        .expect("read rows through the chosen partial-index query predicate")
        .iter()
        .map(|row| {
            (
                row.get::<_, i32>(0),
                row.get::<_, Option<i32>>(1),
                row.get::<_, bool>(2),
                row.get::<_, String>(3),
            )
        })
        .collect()
}

#[test]
fn should_preserve_partial_index_query_results_for_null_excluded_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_index_query_equivalence").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_partial_index_equivalence_rows \
                   (id INT PRIMARY KEY, key_value INT, active BOOLEAN NOT NULL, payload TEXT)",
                "INSERT INTO compat_partial_index_equivalence_rows VALUES \
                   (1, NULL, TRUE, 'active-null'), (2, NULL, FALSE, 'excluded-null'), \
                   (3, 5, TRUE, 'active-five-a'), (4, 5, FALSE, 'excluded-five'), \
                   (5, 5, TRUE, 'active-five-b'), (6, 9, TRUE, 'active-nine')",
            ],
        )
        .await
        .expect("create active and excluded rows with both NULL and non-NULL keys");

        let active_before =
            query_partial_index_rows(&client, "active", "key_value ASC NULLS FIRST, id").await;
        let nulls_before =
            query_partial_index_rows(&client, "key_value IS NULL", "active DESC, id").await;

        // Act
        client
            .batch_execute(
                "CREATE INDEX compat_partial_index_equivalence_idx \
                 ON compat_partial_index_equivalence_rows \
                 (key_value ASC NULLS FIRST) INCLUDE (payload) WHERE active",
            )
            .await
            .expect("create a partial index that includes active NULL keys");
        let active_after_create =
            query_partial_index_rows(&client, "active", "key_value ASC NULLS FIRST, id").await;
        let nulls_after_create =
            query_partial_index_rows(&client, "key_value IS NULL", "active DESC, id").await;
        client
            .batch_execute("REINDEX INDEX compat_partial_index_equivalence_idx")
            .await
            .expect("rebuild the partial index");
        let active_after_reindex =
            query_partial_index_rows(&client, "active", "key_value ASC NULLS FIRST, id").await;
        let nulls_after_reindex =
            query_partial_index_rows(&client, "key_value IS NULL", "active DESC, id").await;
        client
            .batch_execute("DROP INDEX compat_partial_index_equivalence_idx")
            .await
            .expect("remove the partial index after rebuilding it");
        let active_after_drop =
            query_partial_index_rows(&client, "active", "key_value ASC NULLS FIRST, id").await;
        let nulls_after_drop =
            query_partial_index_rows(&client, "key_value IS NULL", "active DESC, id").await;

        // Assert
        let expected_active = vec![
            (1, None, true, "active-null".to_owned()),
            (3, Some(5), true, "active-five-a".to_owned()),
            (5, Some(5), true, "active-five-b".to_owned()),
            (6, Some(9), true, "active-nine".to_owned()),
        ];
        let expected_nulls = vec![
            (1, None, true, "active-null".to_owned()),
            (2, None, false, "excluded-null".to_owned()),
        ];
        assert_eq!(active_before, expected_active);
        assert_eq!(active_after_create, expected_active);
        assert_eq!(active_after_reindex, expected_active);
        assert_eq!(active_after_drop, expected_active);
        assert_eq!(nulls_before, expected_nulls);
        assert_eq!(nulls_after_create, expected_nulls);
        assert_eq!(nulls_after_reindex, expected_nulls);
        assert_eq!(nulls_after_drop, expected_nulls);

        drop(client);
        server.shutdown(connection).await;
    });
}
