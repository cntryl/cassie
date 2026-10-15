fn assert_generated_statistics_copy(
    included_generated: &tokio_postgres::Row,
    excluded_generated: &tokio_postgres::Row,
    included_statistics: &tokio_postgres::Row,
    excluded_statistics: i64,
    included_value: i32,
    excluded_value: i32,
) {
    assert_eq!(included_generated.get::<_, String>(0), "s");
    assert_eq!(excluded_generated.get::<_, String>(0), "");
    assert_eq!(included_statistics.get::<_, String>(0), "{d,f,m}");
    assert_eq!(included_statistics.get::<_, String>(1), "3 4");
    assert_eq!(excluded_statistics, 0);
    assert_eq!(included_value, 8);
    assert_eq!(excluded_value, 999);
}

struct GeneratedStatisticsObservations {
    included_generated: tokio_postgres::Row,
    excluded_generated: tokio_postgres::Row,
    included_statistics: tokio_postgres::Row,
    excluded_statistics: i64,
    included_value: i32,
    excluded_value: i32,
}

async fn observe_generated_statistics_copy(
    client: &tokio_postgres::Client,
) -> GeneratedStatisticsObservations {
    let included_generated = client
        .query_one(
            "SELECT attgenerated::TEXT FROM pg_attribute \
             WHERE attrelid = 'compat_like_generated_included'::regclass \
               AND attname = 'derived'",
            &[],
        )
        .await
        .expect("inspect included generated-column metadata");
    let excluded_generated = client
        .query_one(
            "SELECT attgenerated::TEXT FROM pg_attribute \
             WHERE attrelid = 'compat_like_generated_excluded'::regclass \
               AND attname = 'derived'",
            &[],
        )
        .await
        .expect("inspect excluded generated-column metadata");
    let included_statistics = client
        .query_one(
            "SELECT stxkind::TEXT, stxkeys::TEXT FROM pg_statistic_ext \
             WHERE stxrelid = 'compat_like_generated_included'::regclass",
            &[],
        )
        .await
        .expect("inspect copied extended-statistics definition");
    let excluded_statistics = client
        .query_one(
            "SELECT count(*) FROM pg_statistic_ext \
             WHERE stxrelid = 'compat_like_generated_excluded'::regclass",
            &[],
        )
        .await
        .expect("count excluded extended-statistics definitions")
        .get(0);
    let included_value = client
        .query_one(
            "INSERT INTO compat_like_generated_included (base, category, region) \
             VALUES (4, 'a', 'north') RETURNING derived",
            &[],
        )
        .await
        .expect("evaluate the copied generated expression")
        .get(0);
    let excluded_value = client
        .query_one(
            "INSERT INTO compat_like_generated_excluded \
               (base, derived, category, region) \
             VALUES (4, 999, 'a', 'north') RETURNING derived",
            &[],
        )
        .await
        .expect("insert into column whose generation was excluded")
        .get(0);
    GeneratedStatisticsObservations {
        included_generated,
        excluded_generated,
        included_statistics,
        excluded_statistics,
        included_value,
        excluded_value,
    }
}

#[test]
fn should_copy_generated_statistics_metadata_from_like_source() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_generated_statistics").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_generated_source (\
                   base INT, \
                   derived INT GENERATED ALWAYS AS (base * 2) STORED, \
                   category TEXT, \
                   region TEXT\
                 )",
                "CREATE STATISTICS compat_like_generated_stats \
                   (dependencies, ndistinct, mcv) ON category, region \
                   FROM compat_like_generated_source",
                "CREATE TABLE compat_like_generated_included (\
                   LIKE compat_like_generated_source INCLUDING GENERATED \
                   INCLUDING STATISTICS\
                 )",
                "CREATE TABLE compat_like_generated_excluded (\
                   LIKE compat_like_generated_source INCLUDING ALL \
                   EXCLUDING GENERATED EXCLUDING STATISTICS\
                 )",
            ],
        )
        .await
        .expect("create source and LIKE copies with generated/statistics options");

        // Act
        let observations = observe_generated_statistics_copy(&client).await;

        // Assert
        assert_generated_statistics_copy(
            &observations.included_generated,
            &observations.excluded_generated,
            &observations.included_statistics,
            observations.excluded_statistics,
            observations.included_value,
            observations.excluded_value,
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_create_table_as_values_with_result_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_values_metadata").await;
        let (client, connection) = server.connect().await;

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_values_copy (copy_id, copy_label) AS \
                 VALUES (1, 'one'), (2, 'two')",
            )
            .await;
        let metadata = if created.is_ok() {
            Some(
                client
                    .query(
                        "SELECT attname, atttypid::INT, atttypmod, attnotnull \
                         FROM pg_attribute \
                         WHERE attrelid = 'compat_ctas_values_copy'::regclass \
                           AND attnum > 0 \
                         ORDER BY attnum",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let rows = if created.is_ok() {
            Some(
                client
                    .query(
                        "SELECT copy_id, copy_label FROM compat_ctas_values_copy \
                         ORDER BY copy_id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        let create_sqlstate = created
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;

        assert!(
            created.is_ok(),
            "CTAS AS VALUES should materialize its rows and types; SQLSTATE: {create_sqlstate:?}"
        );
        let metadata = metadata
            .expect("successful CTAS should expose result metadata")
            .expect("read VALUES CTAS metadata");
        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata[0].get::<_, String>(0), "copy_id");
        assert_eq!(metadata[0].get::<_, i32>(1), 23);
        assert_eq!(metadata[0].get::<_, i32>(2), -1);
        assert!(!metadata[0].get::<_, bool>(3));
        assert_eq!(metadata[1].get::<_, String>(0), "copy_label");
        assert_eq!(metadata[1].get::<_, i32>(1), 25);
        assert_eq!(metadata[1].get::<_, i32>(2), -1);
        assert!(!metadata[1].get::<_, bool>(3));
        let rows = rows
            .expect("successful CTAS should be queryable")
            .expect("read VALUES CTAS rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "two");
    });
}
