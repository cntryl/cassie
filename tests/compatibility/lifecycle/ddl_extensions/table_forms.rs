use super::*;

#[test]
fn should_create_table_as_an_existing_table_with_column_names() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_as_table_column_names").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_ctas_table_source (id INT, label TEXT NOT NULL)",
                "INSERT INTO compat_ctas_table_source VALUES (1, 'one'), (2, 'two')",
            ],
        )
        .await
        .expect("create and seed a CTAS table source");

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_table_copy (copy_id, copy_label) \
                 AS TABLE compat_ctas_table_source",
            )
            .await;
        let copied_rows = if created.is_ok() {
            Some(
                client
                    .query("SELECT * FROM compat_ctas_table_copy ORDER BY copy_id", &[])
                    .await,
            )
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "CREATE TABLE AS TABLE with an explicit column-name list should succeed: {created:?}"
        );
        let copied_rows = copied_rows
            .expect("successful CTAS should be queryable")
            .expect("read copied table rows");
        assert_eq!(copied_rows.len(), 2);
        assert_eq!(copied_rows[0].columns()[0].name(), "copy_id");
        assert_eq!(copied_rows[0].columns()[0].type_().oid(), 23);
        assert_eq!(copied_rows[0].columns()[1].name(), "copy_label");
        assert_eq!(copied_rows[0].columns()[1].type_().oid(), 25);
        assert_eq!(copied_rows[0].get::<_, i32>(0), 1);
        assert_eq!(copied_rows[0].get::<_, String>(1), "one");
        assert_eq!(copied_rows[1].get::<_, i32>(0), 2);
        assert_eq!(copied_rows[1].get::<_, String>(1), "two");
    });
}

#[test]
fn should_preserve_unlogged_table_persistence_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unlogged_table_persistence").await;
        let (client, connection) = server.connect().await;
        let created = client
            .batch_execute(
                "CREATE UNLOGGED TABLE compat_unlogged_rows \
                 (id INT PRIMARY KEY, label TEXT)",
            )
            .await;

        // Act
        let (inserted, persistence, rows) = if created.is_ok() {
            let inserted = client
                .execute(
                    "INSERT INTO compat_unlogged_rows VALUES (1, 'ephemeral')",
                    &[],
                )
                .await;
            let persistence = if inserted.is_ok() {
                Some(
                    client
                        .query_one(
                            "SELECT relpersistence::TEXT FROM pg_catalog.pg_class \
                             WHERE relname = 'compat_unlogged_rows'",
                            &[],
                        )
                        .await,
                )
            } else {
                None
            };
            let rows = if inserted.is_ok() {
                Some(
                    client
                        .query(
                            "SELECT id, label FROM compat_unlogged_rows ORDER BY id",
                            &[],
                        )
                        .await,
                )
            } else {
                None
            };
            (Some(inserted), persistence, rows)
        } else {
            (None, None, None)
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "CREATE UNLOGGED TABLE should retain PostgreSQL persistence semantics: {created:?}"
        );
        assert_eq!(
            inserted
                .expect("insert should be attempted after table creation")
                .expect("insert into unlogged table"),
            1
        );
        assert_eq!(
            persistence
                .expect("read relpersistence after inserting")
                .expect("read unlogged table persistence")
                .try_get::<_, String>(0)
                .expect("relpersistence text"),
            "u"
        );
        let rows = rows
            .expect("read rows after inserting")
            .expect("read rows from unlogged table");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "ephemeral");
    });
}

#[test]
fn should_apply_like_all_with_explicit_exclusions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_all_explicit_exclusions").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_options_source (\
                   id INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\
                   label TEXT NOT NULL DEFAULT 'template',\
                   CONSTRAINT compat_like_options_check CHECK (label <> 'blocked'),\
                   UNIQUE (label)\
                 )",
                "COMMENT ON COLUMN compat_like_options_source.label IS 'copied comment'",
                "ALTER TABLE compat_like_options_source ALTER COLUMN label SET STORAGE EXTERNAL",
                "ALTER TABLE compat_like_options_source ALTER COLUMN label SET COMPRESSION pglz",
                "CREATE TABLE compat_like_options_all (\
                   LIKE compat_like_options_source INCLUDING ALL EXCLUDING IDENTITY\
                 )",
                "CREATE TABLE compat_like_options_excluded (\
                   LIKE compat_like_options_source INCLUDING ALL EXCLUDING DEFAULTS\
                   EXCLUDING CONSTRAINTS EXCLUDING INDEXES EXCLUDING IDENTITY\
                 )",
            ],
        )
        .await
        .expect("create LIKE source and combined option copies");

        // Act
        let copied_default = client
            .query_one(
                "INSERT INTO compat_like_options_all (id) VALUES (1) RETURNING label",
                &[],
            )
            .await
            .expect("INCLUDING ALL should copy the source default");
        let duplicate_all_label = client
            .execute(
                "INSERT INTO compat_like_options_all (id, label) VALUES (2, 'template')",
                &[],
            )
            .await
            .expect_err("INCLUDING ALL should copy the unique index");
        let excluded_rows = client
            .execute(
                "INSERT INTO compat_like_options_excluded (id, label) \
                 VALUES (1, 'blocked'), (1, 'blocked')",
                &[],
            )
            .await
            .expect("EXCLUDING options should omit defaults, checks, and indexes");
        let all_label_metadata = client
            .query_one(
                "SELECT pg_get_expr(d.adbin, d.adrelid), \
                        col_description(c.oid, a.attnum), \
                        a.attstorage::TEXT, a.attcompression::TEXT \
                 FROM pg_class c \
                 JOIN pg_attribute a ON a.attrelid = c.oid \
                 LEFT JOIN pg_attrdef d ON d.adrelid = c.oid AND d.adnum = a.attnum \
                 WHERE c.relname = 'compat_like_options_all' AND a.attname = 'label'",
                &[],
            )
            .await
            .expect("inspect included column metadata");
        let excluded_label_default = client
            .query_one(
                "SELECT pg_get_expr(d.adbin, d.adrelid) \
                 FROM pg_class c \
                 JOIN pg_attribute a ON a.attrelid = c.oid \
                 LEFT JOIN pg_attrdef d ON d.adrelid = c.oid AND d.adnum = a.attnum \
                 WHERE c.relname = 'compat_like_options_excluded' AND a.attname = 'label'",
                &[],
            )
            .await
            .expect("inspect excluded default metadata");
        let all_identity = client
            .query_one(
                "SELECT attidentity::TEXT FROM pg_attribute \
                 WHERE attrelid = 'compat_like_options_all'::regclass AND attname = 'id'",
                &[],
            )
            .await
            .expect("inspect excluded identity metadata");
        let all_checks: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_constraint \
                 WHERE conrelid = 'compat_like_options_all'::regclass AND contype = 'c'",
                &[],
            )
            .await
            .expect("count included check constraints")
            .try_get(0)
            .expect("included check count");
        let excluded_checks: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_constraint \
                 WHERE conrelid = 'compat_like_options_excluded'::regclass AND contype = 'c'",
                &[],
            )
            .await
            .expect("count excluded check constraints")
            .try_get(0)
            .expect("excluded check count");
        let all_indexes: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_index \
                 WHERE indrelid = 'compat_like_options_all'::regclass",
                &[],
            )
            .await
            .expect("count included indexes")
            .try_get(0)
            .expect("included index count");
        let excluded_indexes: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_index \
                 WHERE indrelid = 'compat_like_options_excluded'::regclass",
                &[],
            )
            .await
            .expect("count excluded indexes")
            .try_get(0)
            .expect("excluded index count");

        // Assert
        assert_eq!(copied_default.get::<_, String>(0), "template");
        assert_eq!(db_error(&duplicate_all_label).code().code(), "23505");
        assert_eq!(excluded_rows, 2);
        assert_eq!(all_label_metadata.get::<_, String>(0), "'template'::text");
        assert_eq!(all_label_metadata.get::<_, String>(1), "copied comment");
        assert_eq!(all_label_metadata.get::<_, String>(2), "e");
        assert_eq!(all_label_metadata.get::<_, String>(3), "p");
        assert_eq!(excluded_label_default.get::<_, Option<String>>(0), None);
        assert_eq!(all_identity.get::<_, String>(0), "");
        assert_eq!(all_checks, 1);
        assert_eq!(excluded_checks, 0);
        assert_eq!(all_indexes, 2);
        assert_eq!(excluded_indexes, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_ctas_type_modifiers_without_materializing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_type_modifiers_no_data").await;
        let (client, connection) = server.connect().await;

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_metadata (amount, label) AS \
                 SELECT 12::NUMERIC(8, 3), 'short'::VARCHAR(7) \
                 WHERE FALSE WITH NO DATA",
            )
            .await;
        let metadata = if created.is_ok() {
            Some(
                client
                    .query(
                        "SELECT a.attname, a.atttypid::INT, a.atttypmod, a.attnotnull \
                         FROM pg_attribute a \
                         WHERE a.attrelid = 'compat_ctas_metadata'::regclass \
                           AND a.attnum > 0 \
                         ORDER BY a.attnum",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let row_count = if created.is_ok() {
            Some(
                client
                    .query_one("SELECT count(*) FROM compat_ctas_metadata", &[])
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
            "CTAS WITH NO DATA should preserve result type modifiers; SQLSTATE: {create_sqlstate:?}"
        );
        let metadata = metadata
            .expect("successful CTAS should expose catalog metadata")
            .expect("read CTAS catalog metadata");
        assert_eq!(metadata.len(), 2);
        assert_eq!(metadata[0].get::<_, String>(0), "amount");
        assert_eq!(metadata[0].get::<_, i32>(1), 1700);
        assert_eq!(metadata[0].get::<_, i32>(2), 524_295);
        assert!(!metadata[0].get::<_, bool>(3));
        assert_eq!(metadata[1].get::<_, String>(0), "label");
        assert_eq!(metadata[1].get::<_, i32>(1), 1043);
        assert_eq!(metadata[1].get::<_, i32>(2), 11);
        assert!(!metadata[1].get::<_, bool>(3));
        assert_eq!(
            row_count
                .expect("successful CTAS should remain queryable")
                .expect("count rows in CTAS table")
                .get::<_, i64>(0),
            0
        );
    });
}

#[test]
fn should_track_materialized_view_population_through_refresh_no_data() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_population_state").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_population_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_population_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_mv_population (item_id, item_label) AS \
                 SELECT id, label FROM compat_mv_population_source WITH DATA",
            ],
        )
        .await
        .expect("create populated materialized view");

        // Act
        let initially_populated: bool = client
            .query_one(
                "SELECT ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_population'",
                &[],
            )
            .await
            .expect("read initial materialized-view state")
            .try_get(0)
            .expect("initial ispopulated value");
        client
            .execute(
                "INSERT INTO compat_mv_population_source VALUES (3, 'three')",
                &[],
            )
            .await
            .expect("add a source row before refreshing");
        client
            .batch_execute("REFRESH MATERIALIZED VIEW compat_mv_population WITH NO DATA")
            .await
            .expect("mark materialized view unpopulated");
        let unpopulated: bool = client
            .query_one(
                "SELECT ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_population'",
                &[],
            )
            .await
            .expect("read unpopulated materialized-view state")
            .try_get(0)
            .expect("unpopulated ispopulated value");
        let scan_error = client
            .query("SELECT item_id FROM compat_mv_population", &[])
            .await
            .expect_err("an unpopulated materialized view cannot be scanned");
        client
            .batch_execute("REFRESH MATERIALIZED VIEW compat_mv_population WITH DATA")
            .await
            .expect("repopulate materialized view");
        let repopulated: bool = client
            .query_one(
                "SELECT ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_population'",
                &[],
            )
            .await
            .expect("read repopulated materialized-view state")
            .try_get(0)
            .expect("repopulated ispopulated value");
        let rows = client
            .query(
                "SELECT item_id, item_label FROM compat_mv_population ORDER BY item_id",
                &[],
            )
            .await
            .expect("read refreshed materialized-view rows");

        // Assert
        assert!(initially_populated);
        assert!(!unpopulated);
        assert_eq!(db_error(&scan_error).code().code(), "55000");
        assert!(repopulated);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "two");
        assert_eq!(rows[2].get::<_, i32>(0), 3);
        assert_eq!(rows[2].get::<_, String>(1), "three");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_existing_materialized_view_when_created_if_not_exists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_if_not_exists").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_if_not_exists_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_if_not_exists_source VALUES (1, 'original')",
                "CREATE MATERIALIZED VIEW IF NOT EXISTS compat_mv_if_not_exists \
                   (view_id, view_label) AS \
                   SELECT id, label FROM compat_mv_if_not_exists_source WITH DATA",
                "CREATE MATERIALIZED VIEW IF NOT EXISTS compat_mv_if_not_exists \
                   (view_id, view_label) AS SELECT 999, 'replacement' WITH DATA",
            ],
        )
        .await
        .expect("create materialized view and skip duplicate IF NOT EXISTS form");

        // Act
        let original_rows = client
            .query(
                "SELECT view_id, view_label FROM compat_mv_if_not_exists ORDER BY view_id",
                &[],
            )
            .await
            .expect("read existing materialized-view contents");
        let catalog_entry = client
            .query_one(
                "SELECT matviewname, ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_if_not_exists'",
                &[],
            )
            .await
            .expect("inspect materialized-view catalog row");

        // Assert
        assert_eq!(original_rows.len(), 1);
        assert_eq!(original_rows[0].get::<_, i32>(0), 1);
        assert_eq!(original_rows[0].get::<_, String>(1), "original");
        assert_eq!(catalog_entry.get::<_, String>(0), "compat_mv_if_not_exists");
        assert!(catalog_entry.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_track_materialized_view_rename_in_pg_matviews() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_rename_catalog").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE MATERIALIZED VIEW compat_mv_rename AS SELECT 1 AS view_id")
            .await
            .expect("create materialized view before renaming");

        // Act
        client
            .batch_execute(
                "ALTER MATERIALIZED VIEW compat_mv_rename \
                 RENAME TO compat_mv_renamed",
            )
            .await
            .expect("rename materialized view");
        let catalog = client
            .query_one(
                "SELECT matviewname, ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_renamed'",
                &[],
            )
            .await
            .expect("inspect renamed materialized-view catalog row");

        // Assert
        assert_eq!(catalog.get::<_, String>(0), "compat_mv_renamed");
        assert!(catalog.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_remove_dropped_materialized_view_from_pg_matviews() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_drop_catalog").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE MATERIALIZED VIEW compat_mv_drop AS SELECT 1 AS view_id")
            .await
            .expect("create materialized view before dropping");

        // Act
        client
            .batch_execute("DROP MATERIALIZED VIEW compat_mv_drop")
            .await
            .expect("drop materialized view");
        let catalog_rows: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_matviews WHERE matviewname = 'compat_mv_drop'",
                &[],
            )
            .await
            .expect("inspect catalog after materialized-view drop")
            .try_get(0)
            .expect("remaining materialized-view rows");

        // Assert
        assert_eq!(catalog_rows, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_require_a_unique_index_for_concurrent_materialized_view_refresh() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_concurrent_refresh").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_concurrent_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_concurrent_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_mv_concurrent (item_id, item_label) AS \
                   SELECT id, label FROM compat_mv_concurrent_source WITH DATA",
            ],
        )
        .await
        .expect("create materialized view for concurrent refresh");

        // Act
        let missing_unique_index = client
            .batch_execute("REFRESH MATERIALIZED VIEW CONCURRENTLY compat_mv_concurrent")
            .await
            .expect_err("concurrent refresh requires a full-row unique index");
        client
            .batch_execute(
                "CREATE UNIQUE INDEX compat_mv_concurrent_id_uq \
                 ON compat_mv_concurrent (item_id)",
            )
            .await
            .expect("create the required unique index");
        client
            .execute(
                "UPDATE compat_mv_concurrent_source SET label = 'updated' WHERE id = 2",
                &[],
            )
            .await
            .expect("change a source row before concurrent refresh");
        client
            .batch_execute("REFRESH MATERIALIZED VIEW CONCURRENTLY compat_mv_concurrent")
            .await
            .expect("refresh materialized view concurrently after indexing");
        let catalog = client
            .query_one(
                "SELECT hasindexes, ispopulated FROM pg_matviews \
                 WHERE matviewname = 'compat_mv_concurrent'",
                &[],
            )
            .await
            .expect("inspect concurrently refreshed view state");
        let rows = client
            .query(
                "SELECT item_id, item_label FROM compat_mv_concurrent ORDER BY item_id",
                &[],
            )
            .await
            .expect("read concurrently refreshed rows");

        // Assert
        assert_eq!(db_error(&missing_unique_index).code().code(), "55000");
        assert!(catalog.get::<_, bool>(0));
        assert!(catalog.get::<_, bool>(1));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "one");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "updated");

        drop(client);
        server.shutdown(connection).await;
    });
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
        let excluded_statistics: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_statistic_ext \
                 WHERE stxrelid = 'compat_like_generated_excluded'::regclass",
                &[],
            )
            .await
            .expect("count excluded extended-statistics definitions")
            .try_get(0)
            .expect("excluded statistics count");
        let included_value: i32 = client
            .query_one(
                "INSERT INTO compat_like_generated_included (base, category, region) \
                 VALUES (4, 'a', 'north') RETURNING derived",
                &[],
            )
            .await
            .expect("evaluate the copied generated expression")
            .try_get(0)
            .expect("copied generated value");
        let excluded_value: i32 = client
            .query_one(
                "INSERT INTO compat_like_generated_excluded \
                   (base, derived, category, region) \
                 VALUES (4, 999, 'a', 'north') RETURNING derived",
                &[],
            )
            .await
            .expect("insert into column whose generation was excluded")
            .try_get(0)
            .expect("explicit value after excluding generation");

        // Assert
        assert_eq!(included_generated.get::<_, String>(0), "s");
        assert_eq!(excluded_generated.get::<_, String>(0), "");
        assert_eq!(included_statistics.get::<_, String>(0), "{d,f,m}");
        assert_eq!(included_statistics.get::<_, String>(1), "3 4");
        assert_eq!(excluded_statistics, 0);
        assert_eq!(included_value, 8);
        assert_eq!(excluded_value, 999);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_create_table_as_prepared_execute_with_typed_parameters() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_prepared_execute").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "PREPARE compat_ctas_prepared_source (INT, TEXT) AS \
                 SELECT $1 AS source_id, $2 AS source_label",
            )
            .await
            .expect("prepare typed CTAS source statement");

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_execute_copy (copy_id, copy_label) \
                 AS EXECUTE compat_ctas_prepared_source (18, 'prepared')",
            )
            .await;
        let metadata = if created.is_ok() {
            Some(
                client
                    .query(
                        "SELECT attname, atttypid::INT, atttypmod, attnotnull \
                         FROM pg_attribute \
                         WHERE attrelid = 'compat_ctas_execute_copy'::regclass \
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
                        "SELECT copy_id, copy_label FROM compat_ctas_execute_copy",
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
            "CTAS AS EXECUTE should materialize prepared parameters; SQLSTATE: {create_sqlstate:?}"
        );
        let metadata = metadata
            .expect("successful CTAS should expose result metadata")
            .expect("read prepared CTAS metadata");
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
            .expect("read rows created from prepared statement");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 18);
        assert_eq!(rows[0].get::<_, String>(1), "prepared");
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

#[test]
fn should_skip_existing_relation_for_create_table_as_if_not_exists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_if_not_exists").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_ctas_if_not_exists (copy_id, copy_label) AS \
                 VALUES (1, 'original')",
            )
            .await
            .expect("create initial CTAS relation");

        // Act
        let duplicate = client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS compat_ctas_if_not_exists \
                   (copy_id, copy_label) AS VALUES (99, 'replacement')",
            )
            .await;
        let rows = if duplicate.is_ok() {
            Some(
                client
                    .query(
                        "SELECT copy_id, copy_label FROM compat_ctas_if_not_exists \
                         ORDER BY copy_id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            duplicate.is_ok(),
            "CTAS IF NOT EXISTS should skip the existing relation: {duplicate:?}"
        );
        let rows = rows
            .expect("successful duplicate CTAS should leave the relation queryable")
            .expect("read existing CTAS relation");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "original");
    });
}

#[test]
fn should_keep_persistent_select_into_table_after_creator_disconnects() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("select_into_persistent_table").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute(
                "SELECT 18 AS id, 'persistent'::TEXT AS label \
                 INTO compat_select_into_persistent",
            )
            .await
            .expect("SELECT INTO without TEMP should create a persistent table");

        // Act
        drop(owner);
        owner_connection.abort();
        let _ = owner_connection.await;
        let persistence = observer
            .query_one(
                "SELECT relpersistence::TEXT FROM pg_class \
                 WHERE oid = 'compat_select_into_persistent'::regclass",
                &[],
            )
            .await
            .expect("inspect persistent SELECT INTO relation");
        let rows = observer
            .query(
                "SELECT id, label FROM compat_select_into_persistent ORDER BY id",
                &[],
            )
            .await
            .expect("read SELECT INTO rows from another session");

        // Assert
        assert_eq!(persistence.get::<_, String>(0), "p");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 18);
        assert_eq!(rows[0].get::<_, String>(1), "persistent");

        drop(observer);
        server.shutdown(observer_connection).await;
    });
}
