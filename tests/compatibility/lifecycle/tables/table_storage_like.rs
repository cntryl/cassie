use super::*;

async fn column_storage(
    client: &tokio_postgres::Client,
    table: &str,
) -> Vec<(String, String, String)> {
    let sql = format!(
        "SELECT attname::TEXT, attstorage::TEXT, attcompression::TEXT \
         FROM pg_catalog.pg_attribute \
         WHERE attrelid = '{table}'::regclass AND attnum > 0 ORDER BY attnum"
    );
    client
        .query(sql.as_str(), &[])
        .await
        .expect("read column storage metadata")
        .iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect()
}

fn entry(name: &str, storage: &str, compression: &str) -> (String, String, String) {
    (name.to_owned(), storage.to_owned(), compression.to_owned())
}

#[test]
fn should_report_postgres_default_storage_for_each_column_type() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("column_storage_type_defaults").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute(
                "CREATE TABLE compat_storage_defaults (\
                   a INT, b TEXT, c NUMERIC, d BYTEA, e UUID, f JSONB, g BOOLEAN\
                 )",
            )
            .await
            .expect("create table with default column storage");
        let metadata = column_storage(&client, "compat_storage_defaults").await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            metadata,
            vec![
                entry("a", "p", ""),
                entry("b", "x", ""),
                entry("c", "m", ""),
                entry("d", "x", ""),
                entry("e", "p", ""),
                entry("f", "x", ""),
                entry("g", "p", ""),
            ]
        );
    });
}

#[test]
fn should_alter_column_storage_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_column_storage").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_alter_storage (a TEXT, b NUMERIC, c INT)")
            .await
            .expect("create table for storage changes");
        client
            .batch_execute("INSERT INTO compat_alter_storage VALUES ('kept', 1.5, 7)")
            .await
            .expect("seed row");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_alter_storage ALTER COLUMN a SET STORAGE MAIN, \
                   ALTER COLUMN a SET COMPRESSION lz4",
            )
            .await
            .expect("set storage and compression");
        let changed = column_storage(&client, "compat_alter_storage").await;
        execute_statements(
            &client,
            &[
                "ALTER TABLE compat_alter_storage ALTER COLUMN a SET STORAGE DEFAULT",
                "ALTER TABLE compat_alter_storage ALTER COLUMN a SET COMPRESSION DEFAULT",
            ],
        )
        .await
        .expect("reset storage and compression");
        let reset = column_storage(&client, "compat_alter_storage").await;
        let invalid = client
            .batch_execute("ALTER TABLE compat_alter_storage ALTER COLUMN c SET STORAGE EXTERNAL")
            .await
            .expect_err("fixed-width columns can only be PLAIN");
        let row = client
            .query_one("SELECT a, c FROM compat_alter_storage", &[])
            .await
            .expect("rows are unchanged");

        // Assert
        let sqlstate = db_error(&invalid).code().code().to_owned();
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(changed[0], entry("a", "m", "l"));
        assert_eq!(reset[0], entry("a", "x", ""));
        assert_eq!(sqlstate, "22023");
        assert_eq!(row.get::<_, String>(0), "kept");
        assert_eq!(row.get::<_, i32>(1), 7);
    });
}

#[test]
fn should_roll_back_column_storage_changes_with_the_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("rollback_column_storage").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_rollback_storage (a TEXT STORAGE MAIN)")
            .await
            .expect("create table with storage");

        // Act
        execute_statements(
            &client,
            &[
                "BEGIN",
                "ALTER TABLE compat_rollback_storage ALTER COLUMN a SET STORAGE EXTERNAL",
                "ALTER TABLE compat_rollback_storage ALTER COLUMN a SET COMPRESSION pglz",
                "ROLLBACK",
            ],
        )
        .await
        .expect("roll back storage changes");
        let after_rollback = column_storage(&client, "compat_rollback_storage").await;
        execute_statements(
            &client,
            &[
                "BEGIN",
                "CREATE TABLE compat_rollback_created (a TEXT COMPRESSION lz4)",
                "ROLLBACK",
            ],
        )
        .await
        .expect("roll back table creation");
        let created_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'compat_rollback_created'",
                &[],
            )
            .await
            .expect("count rolled back relation")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(after_rollback, vec![entry("a", "m", "")]);
        assert_eq!(created_count, 0);
    });
}

#[test]
fn should_copy_column_storage_metadata_only_when_like_includes_it() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_column_storage").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_like_storage_source (\
                   a TEXT STORAGE MAIN COMPRESSION lz4, b NUMERIC COMPRESSION pglz)",
            )
            .await
            .expect("create LIKE source");

        // Act
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_storage_none (LIKE compat_like_storage_source)",
                "CREATE TABLE compat_like_storage_storage \
                   (LIKE compat_like_storage_source INCLUDING STORAGE)",
                "CREATE TABLE compat_like_storage_compression \
                   (LIKE compat_like_storage_source INCLUDING COMPRESSION)",
                "CREATE TABLE compat_like_storage_all \
                   (LIKE compat_like_storage_source INCLUDING ALL)",
            ],
        )
        .await
        .expect("create LIKE targets");
        let none = column_storage(&client, "compat_like_storage_none").await;
        let storage = column_storage(&client, "compat_like_storage_storage").await;
        let compression = column_storage(&client, "compat_like_storage_compression").await;
        let all = column_storage(&client, "compat_like_storage_all").await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(none, vec![entry("a", "x", ""), entry("b", "m", "")]);
        assert_eq!(storage, vec![entry("a", "m", ""), entry("b", "m", "")]);
        assert_eq!(
            compression,
            vec![entry("a", "x", "l"), entry("b", "m", "p")]
        );
        assert_eq!(all, vec![entry("a", "m", "l"), entry("b", "m", "p")]);
    });
}

#[test]
fn should_copy_column_comments_only_when_like_includes_them() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_column_comments").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_comment_source (id INT, label TEXT)",
                "COMMENT ON COLUMN compat_like_comment_source.label IS 'visible label'",
                "COMMENT ON TABLE compat_like_comment_source IS 'table comment'",
                "CREATE TABLE compat_like_comment_none (LIKE compat_like_comment_source)",
                "CREATE TABLE compat_like_comment_copy \
                   (LIKE compat_like_comment_source INCLUDING COMMENTS)",
                "CREATE TABLE compat_like_comment_all \
                   (LIKE compat_like_comment_source INCLUDING ALL EXCLUDING COMMENTS)",
            ],
        )
        .await
        .expect("create LIKE comment fixtures");

        // Act
        let mut descriptions = Vec::new();
        for table in [
            "compat_like_comment_none",
            "compat_like_comment_copy",
            "compat_like_comment_all",
        ] {
            let column: Option<String> = client
                .query_one("SELECT col_description($1::TEXT::regclass, 2)", &[&table])
                .await
                .expect("read column comment")
                .get(0);
            let relation: Option<String> = client
                .query_one(
                    "SELECT obj_description($1::TEXT::regclass, 'pg_class')",
                    &[&table],
                )
                .await
                .expect("read table comment")
                .get(0);
            descriptions.push((column, relation));
        }

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(descriptions[0], (None, None));
        assert_eq!(descriptions[1], (Some("visible label".to_owned()), None));
        assert_eq!(descriptions[2], (None, None));
    });
}

#[test]
fn should_accept_like_generated_options_as_noops_without_extra_state() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_generated_statistics").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_like_noop_source (id INT, label TEXT)")
            .await
            .expect("create LIKE source");

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_like_noop_target (\
                   LIKE compat_like_noop_source INCLUDING GENERATED INCLUDING STATISTICS)",
            )
            .await;
        let generated: Vec<String> = client
            .query(
                "SELECT attgenerated::TEXT FROM pg_catalog.pg_attribute \
                 WHERE attrelid = 'compat_like_noop_target'::regclass AND attnum > 0",
                &[],
            )
            .await
            .expect("read generated flags")
            .iter()
            .map(|row| row.get(0))
            .collect();

        // Assert
        let sqlstate = created
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(sqlstate, None);
        assert_eq!(generated, vec![String::new(), String::new()]);
    });
}

#[test]
fn should_share_the_source_sequence_when_like_includes_sequence_defaults() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_sequence_default").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SEQUENCE compat_like_shared_seq",
                "CREATE TABLE compat_like_seq_source \
                   (id INT DEFAULT nextval('compat_like_shared_seq'), label TEXT)",
                "CREATE TABLE compat_like_seq_serial (id SERIAL, label TEXT)",
                "CREATE TABLE compat_like_seq_target \
                   (LIKE compat_like_seq_source INCLUDING DEFAULTS)",
                "CREATE TABLE compat_like_seq_serial_copy \
                   (LIKE compat_like_seq_serial INCLUDING DEFAULTS)",
                "CREATE TABLE compat_like_seq_plain (LIKE compat_like_seq_source)",
            ],
        )
        .await
        .expect("create sequence-backed LIKE fixtures");

        // Act
        client
            .batch_execute(
                "INSERT INTO compat_like_seq_source (label) VALUES ('source'); \
                 INSERT INTO compat_like_seq_target (label) VALUES ('target'); \
                 INSERT INTO compat_like_seq_source (label) VALUES ('source again'); \
                 INSERT INTO compat_like_seq_serial (label) VALUES ('serial'); \
                 INSERT INTO compat_like_seq_serial_copy (label) VALUES ('copy'); \
                 INSERT INTO compat_like_seq_plain (id, label) VALUES (NULL, 'plain')",
            )
            .await
            .expect("insert through shared sequence defaults");
        let shared = client
            .query("SELECT id FROM compat_like_seq_source ORDER BY id", &[])
            .await
            .expect("read source ids");
        let target = client
            .query_one("SELECT id FROM compat_like_seq_target", &[])
            .await
            .expect("read target id");
        let serial = client
            .query_one("SELECT id FROM compat_like_seq_serial_copy", &[])
            .await
            .expect("read serial copy id");
        let plain: Option<i32> = client
            .query_one("SELECT id FROM compat_like_seq_plain", &[])
            .await
            .expect("read plain copy id")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(shared.len(), 2);
        assert_eq!(shared[0].get::<_, i32>(0), 1);
        assert_eq!(shared[1].get::<_, i32>(0), 3);
        assert_eq!(target.get::<_, i32>(0), 2);
        assert_eq!(serial.get::<_, i32>(0), 2);
        assert_eq!(plain, None);
    });
}

#[test]
fn should_copy_full_text_indexes_with_target_owned_names_when_like_includes_indexes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_fulltext_index").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_ft_source (id INT, body TEXT)",
                "CREATE INDEX compat_like_ft_idx ON compat_like_ft_source USING fulltext (body)",
                "CREATE TABLE compat_like_ft_target \
                   (LIKE compat_like_ft_source INCLUDING INDEXES)",
            ],
        )
        .await
        .expect("create full-text LIKE fixtures");

        // Act
        client
            .batch_execute("INSERT INTO compat_like_ft_target VALUES (1, 'searchable body')")
            .await
            .expect("write through the cloned full-text index");
        client
            .batch_execute("DROP INDEX compat_like_ft_idx")
            .await
            .expect("drop the source index independently");
        let names: Vec<(String, String)> = client
            .query(
                "SELECT tablename::TEXT, indexname::TEXT FROM pg_catalog.pg_indexes \
                 WHERE tablename IN ('compat_like_ft_source', 'compat_like_ft_target') \
                 ORDER BY tablename, indexname",
                &[],
            )
            .await
            .expect("read index ownership")
            .iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect();

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].0, "compat_like_ft_target");
        assert_ne!(names[0].1, "compat_like_ft_idx");
    });
}

#[test]
fn should_reject_vector_index_clones_before_publishing_the_like_target() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_vector_index_rejected").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE EXTENSION vector",
                "CREATE TABLE compat_like_vec_source (id INT, embedding VECTOR(3))",
                "CREATE INDEX compat_like_vec_idx ON compat_like_vec_source \
                   USING hnsw (embedding vector_l2_ops)",
            ],
        )
        .await
        .expect("create vector LIKE fixtures");

        // Act
        let error = client
            .batch_execute(
                "CREATE TABLE compat_like_vec_target \
                 (LIKE compat_like_vec_source INCLUDING INDEXES)",
            )
            .await
            .expect_err("vector index cloning is deterministically rejected");
        let published: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class WHERE relname = 'compat_like_vec_target'",
                &[],
            )
            .await
            .expect("check the target was not published")
            .get(0);

        // Assert
        let sqlstate = db_error(&error).code().code().to_owned();
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(sqlstate, "0A000");
        assert_eq!(published, 0);
    });
}

#[test]
fn should_join_materialized_views_to_the_heap_access_method() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("matview_relam").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE MATERIALIZED VIEW compat_relam_plain AS SELECT 1 AS value",
                "CREATE MATERIALIZED VIEW compat_relam_heap USING heap AS SELECT 2 AS value",
            ],
        )
        .await
        .expect("create materialized views");

        // Act
        let rows = client
            .query(
                "SELECT relation.relname::TEXT, relation.relam::INT, access_method.amname::TEXT \
                 FROM pg_catalog.pg_class AS relation \
                 JOIN pg_catalog.pg_am AS access_method ON access_method.oid = relation.relam \
                 WHERE relation.relkind = 'm' ORDER BY relation.relname",
                &[],
            )
            .await
            .expect("join materialized views to pg_am");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(rows.len(), 2);
        for row in &rows {
            assert_eq!(row.get::<_, i32>(1), 2);
            assert_eq!(row.get::<_, String>(2), "heap");
        }
    });
}

#[test]
fn should_reject_other_materialized_view_access_methods_with_0a000() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("matview_other_access_method").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE MATERIALIZED VIEW compat_matview_am AS SELECT 1 AS value")
            .await
            .expect("create materialized view");

        // Act
        let create = client
            .batch_execute("CREATE MATERIALIZED VIEW compat_matview_brin USING brin AS SELECT 1")
            .await
            .expect_err("unknown access method is rejected");
        let alter = client
            .batch_execute("ALTER MATERIALIZED VIEW compat_matview_am SET ACCESS METHOD brin")
            .await
            .expect_err("unknown access method is rejected");
        let missing = client
            .batch_execute("ALTER MATERIALIZED VIEW compat_matview_missing SET ACCESS METHOD heap")
            .await
            .expect_err("missing materialized view is rejected");

        // Assert
        let create_state = db_error(&create).code().code().to_owned();
        let alter_state = db_error(&alter).code().code().to_owned();
        let missing_state = db_error(&missing).code().code().to_owned();
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(create_state, "0A000");
        assert_eq!(alter_state, "0A000");
        assert_ne!(missing_state, "00000");
    });
}
