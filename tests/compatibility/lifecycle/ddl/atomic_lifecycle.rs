use super::*;

fn assert_renamed_alter_restart(
    before_catalog: &[Vec<cassie::types::Value>],
    after_catalog: &[Vec<cassie::types::Value>],
    before_rows: &[Vec<cassie::types::Value>],
    after_rows: &[Vec<cassie::types::Value>],
) {
    assert_eq!(after_catalog, before_catalog);
    assert_eq!(after_catalog[0][1], cassie::types::Value::Int64(1700));
    assert_eq!(after_catalog[0][2], cassie::types::Value::Int64(1_310_724));
    assert_eq!(after_rows, before_rows);
    assert_eq!(
        after_rows,
        &vec![
            vec![
                cassie::types::Value::String("1".to_owned()),
                cassie::types::Value::String("41".to_owned()),
                cassie::types::Value::String("persisted".to_owned()),
            ],
            vec![
                cassie::types::Value::String("2".to_owned()),
                cassie::types::Value::Null,
                cassie::types::Value::String("persisted".to_owned()),
            ],
        ]
    );
}
use std::time::Duration;

#[path = "drop_column_cascade.rs"]
mod drop_column_cascade;
#[path = "savepoint_identity.rs"]
mod savepoint_identity;

async fn simple_query_frames(server: &CompatibilityServer, sql: &str) -> Vec<(u8, Vec<u8>)> {
    let socket = tokio::net::TcpStream::connect(server.addr)
        .await
        .expect("connect raw pgwire client");
    let (mut reader, mut writer) = socket.into_split();
    support_pgwire::complete_startup(&mut reader, &mut writer).await;
    support_pgwire::write_frames(&mut writer, vec![support_pgwire::simple_query_frame(sql)]).await;
    support_pgwire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await
}

fn command_tag(frames: &[(u8, Vec<u8>)]) -> Option<String> {
    frames
        .iter()
        .find(|(tag, _)| *tag == b'C')
        .map(|(_, payload)| {
            String::from_utf8_lossy(payload.strip_suffix(&[0]).unwrap_or(payload)).into_owned()
        })
}

fn notice_fields(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<(char, String)>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'N')
        .map(|(_, payload)| support_pgwire::parse_error_fields(payload))
        .collect()
}

fn notice_field(fields: &[(char, String)], kind: char) -> Option<&str> {
    fields
        .iter()
        .find(|(field_kind, _)| *field_kind == kind)
        .map(|(_, value)| value.as_str())
}

#[test]
fn should_apply_using_conversion_with_column_addition_in_one_alter() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_alter_type_add_column").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_issue839_alter_rows \
                 (id INT PRIMARY KEY, amount INT)",
                "INSERT INTO compat_issue839_alter_rows VALUES (1, 22), (2, NULL)",
            ],
        )
        .await
        .expect("create rows with nullable integer values");

        // Act
        let alter_result = client
            .batch_execute(
                "ALTER TABLE compat_issue839_alter_rows \
                 ALTER COLUMN amount TYPE NUMERIC(20,0) USING amount::numeric, \
                 ADD COLUMN note TEXT DEFAULT 'added'",
            )
            .await;
        let rows = if alter_result.is_ok() {
            Some(
                client
                    .query(
                        "SELECT id, amount::TEXT, note \
                         FROM compat_issue839_alter_rows ORDER BY id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let amount_descriptor = if alter_result.is_ok() {
            Some(
                client
                    .query_one("SELECT amount FROM compat_issue839_alter_rows LIMIT 1", &[])
                    .await,
            )
        } else {
            None
        };
        let amount_typmod = if alter_result.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT atttypmod FROM pg_catalog.pg_attribute \
                         WHERE attrelid = 'compat_issue839_alter_rows'::regclass \
                           AND attname = 'amount'",
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
            alter_result.is_ok(),
            "type conversion and column addition should succeed together: {alter_result:?}"
        );
        let rows = rows
            .expect("read converted rows only after ALTER succeeds")
            .expect("read converted rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (
                rows[0].get::<_, i32>(0),
                rows[0].get::<_, String>(1),
                rows[0].get::<_, String>(2),
            ),
            (1, "22".to_owned(), "added".to_owned())
        );
        assert_eq!(
            (
                rows[1].get::<_, i32>(0),
                rows[1].get::<_, Option<String>>(1),
                rows[1].get::<_, String>(2),
            ),
            (2, None, "added".to_owned())
        );
        let amount_descriptor = amount_descriptor
            .expect("read the converted column descriptor after ALTER")
            .expect("describe converted numeric column");
        assert_eq!(amount_descriptor.columns()[0].type_().oid(), 1700);
        let amount_typmod = amount_typmod
            .expect("read the converted column typmod after ALTER")
            .expect("read numeric precision and scale");
        assert_eq!(amount_typmod.get::<_, i32>(0), 1_310_724);
    });
}

#[test]
fn should_rollback_multi_action_alter_after_using_failure() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_alter_failure_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_issue839_alter_atomic (id INT, amount INT)",
                "INSERT INTO compat_issue839_alter_atomic VALUES (1, 22), (2, 24)",
            ],
        )
        .await
        .expect("create existing rows before a failing multi-action ALTER");

        // Act
        let alter_result = client
            .batch_execute(
                "ALTER TABLE compat_issue839_alter_atomic \
                 ADD COLUMN marker TEXT DEFAULT 'temporary', \
                 ALTER COLUMN amount TYPE NUMERIC(20,0) \
                   USING amount::numeric / (id - 1)",
            )
            .await;
        let columns = client
            .query(
                "SELECT column_name, data_type FROM information_schema.columns \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_issue839_alter_atomic' \
                 ORDER BY ordinal_position",
                &[],
            )
            .await
            .expect("inspect table metadata after the failed ALTER");
        let rows = client
            .query(
                "SELECT id, amount FROM compat_issue839_alter_atomic ORDER BY id",
                &[],
            )
            .await
            .expect("read original values after the failed ALTER");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        let error = alter_result.expect_err("USING division by zero must reject the ALTER");
        let database_error = db_error(&error);
        assert_eq!(database_error.code().code(), "22012");
        assert_eq!(database_error.message(), "division by zero");
        assert_eq!(
            columns
                .iter()
                .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1),))
                .collect::<Vec<_>>(),
            vec![
                ("id".to_owned(), "integer".to_owned()),
                ("amount".to_owned(), "integer".to_owned()),
            ],
            "the earlier ADD COLUMN action must not publish when USING fails"
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, i32>(1)))
                .collect::<Vec<_>>(),
            vec![(1, 22), (2, 24)]
        );
    });
}

#[test]
fn should_emit_if_exists_notices_with_command_tags() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_if_exists_notices").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_issue839_drop_present (id INT)",
                "CREATE TABLE compat_issue839_alter_present (id INT)",
            ],
        )
        .await
        .expect("create present objects for IF EXISTS comparisons");

        // Act
        let absent_drop =
            simple_query_frames(&server, "DROP TABLE IF EXISTS compat_issue839_drop_absent").await;
        let present_drop =
            simple_query_frames(&server, "DROP TABLE IF EXISTS compat_issue839_drop_present").await;
        let absent_alter = simple_query_frames(
            &server,
            "ALTER TABLE IF EXISTS compat_issue839_alter_absent ADD COLUMN ignored INT",
        )
        .await;
        let present_alter = simple_query_frames(
            &server,
            "ALTER TABLE IF EXISTS compat_issue839_alter_present \
             ADD COLUMN added INT DEFAULT 7",
        )
        .await;
        // Assert
        for (statement, frames) in [
            ("absent DROP TABLE", &absent_drop),
            ("present DROP TABLE", &present_drop),
            ("absent ALTER TABLE", &absent_alter),
            ("present ALTER TABLE", &present_alter),
        ] {
            assert_eq!(
                support_pgwire::error_code(frames),
                None,
                "{statement} frames: {frames:?}"
            );
        }
        let absent_drop_notices = notice_fields(&absent_drop);
        assert_eq!(absent_drop_notices.len(), 1);
        assert_eq!(notice_field(&absent_drop_notices[0], 'S'), Some("NOTICE"));
        assert_eq!(notice_field(&absent_drop_notices[0], 'C'), Some("00000"));
        assert_eq!(
            notice_field(&absent_drop_notices[0], 'M'),
            Some("table \"compat_issue839_drop_absent\" does not exist, skipping")
        );
        assert_eq!(command_tag(&absent_drop).as_deref(), Some("DROP TABLE"));
        assert_eq!(
            notice_fields(&present_drop),
            Vec::<Vec<(char, String)>>::new()
        );
        assert_eq!(command_tag(&present_drop).as_deref(), Some("DROP TABLE"));

        let absent_alter_notices = notice_fields(&absent_alter);
        assert_eq!(absent_alter_notices.len(), 1);
        assert_eq!(notice_field(&absent_alter_notices[0], 'S'), Some("NOTICE"));
        assert_eq!(notice_field(&absent_alter_notices[0], 'C'), Some("00000"));
        assert_eq!(
            notice_field(&absent_alter_notices[0], 'M'),
            Some("relation \"compat_issue839_alter_absent\" does not exist, skipping")
        );
        assert_eq!(command_tag(&absent_alter).as_deref(), Some("ALTER TABLE"));
        assert_eq!(
            notice_fields(&present_alter),
            Vec::<Vec<(char, String)>>::new()
        );
        assert_eq!(command_tag(&present_alter).as_deref(), Some("ALTER TABLE"));
        let inserted = client
            .query_one(
                "INSERT INTO compat_issue839_alter_present (id) VALUES (1) RETURNING added",
                &[],
            )
            .await
            .expect("verify that the present IF EXISTS ALTER ran");
        assert_eq!(inserted.get::<_, i32>(0), 7);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_relation_index_identity_across_schema_move() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_schema_move_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_issue839_move_source",
                "CREATE SCHEMA compat_issue839_move_target",
                "CREATE TABLE compat_issue839_move_source.rows \
                 (id INT PRIMARY KEY, note TEXT)",
                "CREATE INDEX compat_issue839_move_note_idx \
                 ON compat_issue839_move_source.rows(note)",
                "INSERT INTO compat_issue839_move_source.rows VALUES (1, 'kept')",
            ],
        )
        .await
        .expect("create the source relation and dependent index");
        let before = client
            .query_one(
                "SELECT table_class.oid::BIGINT, index_class.oid::BIGINT, \
                        table_schema.nspname, index_schema.nspname \
                 FROM pg_catalog.pg_class AS table_class \
                 JOIN pg_catalog.pg_namespace AS table_schema \
                   ON table_schema.nspname = table_class.relnamespace \
                 JOIN pg_catalog.pg_index AS index_metadata \
                   ON index_metadata.indrelid = table_class.relname \
                 JOIN pg_catalog.pg_class AS index_class \
                   ON index_class.relname = index_metadata.indexrelid \
                 JOIN pg_catalog.pg_namespace AS index_schema \
                   ON index_schema.nspname = index_class.relnamespace \
                 WHERE table_schema.nspname = 'compat_issue839_move_source' \
                   AND table_class.relname = 'rows' \
                   AND index_class.relname = 'compat_issue839_move_note_idx'",
                &[],
            )
            .await
            .expect("read table and index identities before schema move");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_issue839_move_source.rows \
                 SET SCHEMA compat_issue839_move_target",
            )
            .await
            .expect("move the table and its index to the target schema");
        let after = client
            .query_one(
                "SELECT table_class.oid::BIGINT, index_class.oid::BIGINT, \
                        table_schema.nspname, index_schema.nspname \
                 FROM pg_catalog.pg_class AS table_class \
                 JOIN pg_catalog.pg_namespace AS table_schema \
                   ON table_schema.nspname = table_class.relnamespace \
                 JOIN pg_catalog.pg_index AS index_metadata \
                   ON index_metadata.indrelid = table_class.relname \
                 JOIN pg_catalog.pg_class AS index_class \
                   ON index_class.relname = index_metadata.indexrelid \
                 JOIN pg_catalog.pg_namespace AS index_schema \
                   ON index_schema.nspname = index_class.relnamespace \
                 WHERE table_schema.nspname = 'compat_issue839_move_target' \
                   AND table_class.relname = 'rows' \
                   AND index_class.relname = 'compat_issue839_move_note_idx'",
                &[],
            )
            .await
            .expect("read table and index identities after schema move");
        let old_name_is_missing: bool = client
            .query_one(
                "SELECT to_regclass('compat_issue839_move_source.rows') IS NULL",
                &[],
            )
            .await
            .expect("check the old qualified table name")
            .get(0);
        let rows = client
            .query(
                "SELECT id, note FROM compat_issue839_move_target.rows ORDER BY id",
                &[],
            )
            .await
            .expect("read the moved table data");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(before.get::<_, i64>(0), after.get::<_, i64>(0));
        assert_eq!(before.get::<_, i64>(1), after.get::<_, i64>(1));
        assert_eq!(before.get::<_, String>(2), "compat_issue839_move_source");
        assert_eq!(before.get::<_, String>(3), "compat_issue839_move_source");
        assert_eq!(after.get::<_, String>(2), "compat_issue839_move_target");
        assert_eq!(after.get::<_, String>(3), "compat_issue839_move_target");
        assert!(old_name_is_missing);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "kept");
    });
}

#[test]
fn should_reject_drop_restrict_then_cascade_dependent_view() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_drop_dependency_modes").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_issue839_drop_base (id INT PRIMARY KEY)",
                "INSERT INTO compat_issue839_drop_base VALUES (7)",
                "CREATE VIEW compat_issue839_drop_view \
                 AS SELECT id FROM compat_issue839_drop_base",
                "CREATE TABLE compat_issue839_drop_child \
                 (parent_id INT REFERENCES compat_issue839_drop_base(id))",
            ],
        )
        .await
        .expect("create a table with a dependent view");

        // Act
        let restricted_drop = client
            .batch_execute("DROP TABLE compat_issue839_drop_base RESTRICT")
            .await;
        let dependent_rows = client
            .query("SELECT id FROM compat_issue839_drop_view ORDER BY id", &[])
            .await
            .expect("read the view after RESTRICT rejects the drop");
        let cascade_result = client
            .batch_execute("DROP TABLE compat_issue839_drop_base CASCADE")
            .await;
        let remaining_objects: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname IN \
                   ('compat_issue839_drop_base', 'compat_issue839_drop_view')",
                &[],
            )
            .await
            .expect("inspect both dependent objects after CASCADE")
            .get(0);
        let child_insert = client
            .execute("INSERT INTO compat_issue839_drop_child VALUES (99)", &[])
            .await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        let error = restricted_drop.expect_err("RESTRICT must reject a dependent-object drop");
        let database_error = db_error(&error);
        assert_eq!(database_error.code().code(), "2BP01");
        assert_eq!(
            database_error.message(),
            "cannot drop table compat_issue839_drop_base because other objects depend on it"
        );
        assert!(database_error
            .detail()
            .expect("dependent view detail")
            .contains("compat_issue839_drop_view"));
        assert_eq!(dependent_rows.len(), 1);
        assert_eq!(dependent_rows[0].get::<_, i32>(0), 7);
        assert!(
            cascade_result.is_ok(),
            "CASCADE should remove the dependency: {cascade_result:?}"
        );
        assert_eq!(remaining_objects, 0);
        assert!(
            child_insert.is_ok(),
            "CASCADE should remove the foreign key from the surviving child table: {child_insert:?}"
        );
    });
}

#[test]
fn should_find_dependencies_in_cte_backed_views_when_dropping_tables() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_drop_cte_dependency").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_issue839_cte_base (id INT PRIMARY KEY)",
                "INSERT INTO compat_issue839_cte_base VALUES (11)",
                "CREATE VIEW compat_issue839_cte_view AS \
                 WITH base_rows AS (SELECT id FROM compat_issue839_cte_base) \
                 SELECT id FROM base_rows",
            ],
        )
        .await
        .expect("create a table and a CTE-backed dependent view");

        // Act
        let restricted_drop = client
            .batch_execute("DROP TABLE compat_issue839_cte_base RESTRICT")
            .await;
        let view_rows = client
            .query("SELECT id FROM compat_issue839_cte_view", &[])
            .await;
        let cascade_result = client
            .batch_execute("DROP TABLE compat_issue839_cte_base CASCADE")
            .await;
        let remaining: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_class \
                 WHERE relname IN (\
                   'compat_issue839_cte_base', 'compat_issue839_cte_view'\
                 )",
                &[],
            )
            .await
            .expect("inspect relation state after CASCADE")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        let error = restricted_drop.expect_err("RESTRICT must find the CTE-backed dependency");
        assert_eq!(db_error(&error).code().code(), "2BP01");
        assert_eq!(
            view_rows
                .expect("the view remains queryable after RESTRICT")
                .first()
                .expect("the view retains its row")
                .get::<_, i32>(0),
            11
        );
        cascade_result.expect("CASCADE must remove the dependent CTE-backed view");
        assert_eq!(remaining, 0);
    });
}

#[test]
fn should_preserve_unrelated_view_with_prefixed_relation_name_on_cascade() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("drop_table_cascade_prefix").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_cascade_source (id INT)",
                "CREATE TABLE compat_cascade_source_archive (id INT)",
                "INSERT INTO compat_cascade_source_archive VALUES (13)",
                "CREATE VIEW compat_cascade_source_archive_view \
                 AS SELECT id FROM compat_cascade_source_archive \
                   AS compat_cascade_source",
            ],
        )
        .await
        .expect("create a dropped table and a similarly named independent view");

        // Act
        client
            .batch_execute("DROP TABLE compat_cascade_source CASCADE")
            .await
            .expect("drop the table without dropping similarly named relations");
        let retained_value: i32 = client
            .query_one("SELECT id FROM compat_cascade_source_archive_view", &[])
            .await
            .expect("read the independent view after the cascade")
            .get(0);

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(retained_value, 13);
    });
}

#[test]
fn should_rollback_renamed_table_drop_with_transaction() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_rename_drop_rollback").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_issue839_rollback_source (id INT, label TEXT)",
                "INSERT INTO compat_issue839_rollback_source VALUES (1, 'preserved')",
            ],
        )
        .await
        .expect("create a populated table before transactional rename and drop");

        // Act
        let ddl_result = execute_statements(
            &client,
            &[
                "BEGIN",
                "ALTER TABLE compat_issue839_rollback_source \
                 RENAME TO compat_issue839_rollback_renamed",
                "ALTER TABLE compat_issue839_rollback_renamed \
                 ADD COLUMN transient TEXT DEFAULT 'temporary'",
                "DROP TABLE compat_issue839_rollback_renamed",
            ],
        )
        .await;
        let rollback_result = client.batch_execute("ROLLBACK").await;
        let original_rows = client
            .query(
                "SELECT id, label FROM compat_issue839_rollback_source ORDER BY id",
                &[],
            )
            .await
            .expect("read the original table after rollback");
        let catalog_counts = client
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM pg_catalog.pg_class \
                    WHERE relname = 'compat_issue839_rollback_source' AND relkind = 'r'), \
                   (SELECT count(*) FROM pg_catalog.pg_class \
                    WHERE relname = 'compat_issue839_rollback_renamed' AND relkind = 'r'), \
                   (SELECT count(*) FROM information_schema.columns \
                    WHERE table_name = 'compat_issue839_rollback_source' \
                      AND column_name = 'transient')",
                &[],
            )
            .await
            .expect("inspect relation names and columns after rollback");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            ddl_result.is_ok(),
            "transactional DDL steps: {ddl_result:?}"
        );
        assert!(
            rollback_result.is_ok(),
            "ROLLBACK should restore the original relation"
        );
        assert_eq!(original_rows.len(), 1);
        assert_eq!(original_rows[0].get::<_, i32>(0), 1);
        assert_eq!(original_rows[0].get::<_, String>(1), "preserved");
        assert_eq!(
            (
                catalog_counts.get::<_, i64>(0),
                catalog_counts.get::<_, i64>(1),
                catalog_counts.get::<_, i64>(2),
            ),
            (1, 0, 0)
        );
    });
}

#[test]
fn should_persist_renamed_alter_relation_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("issue839_rename_alter_restart");
    let _directory = DataDirectory(path.clone());
    let before_restart = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create local Cassie instance");
        cassie.startup().expect("start local Cassie instance");
        let session = cassie.create_session("tester", None);
        for statement in [
            "CREATE SCHEMA compat_issue839_restart",
            "CREATE TABLE compat_issue839_restart.rows (id INT, amount INT)",
            "INSERT INTO compat_issue839_restart.rows VALUES (1, 41), (2, NULL)",
            "ALTER TABLE compat_issue839_restart.rows \
             ALTER COLUMN amount TYPE NUMERIC(20,0) USING amount::numeric, \
             ADD COLUMN note TEXT DEFAULT 'persisted'",
            "ALTER TABLE compat_issue839_restart.rows RENAME TO rows_v2",
        ] {
            cassie
                .execute_sql(&session, statement, vec![])
                .unwrap_or_else(|error| {
                    panic!("execute DDL before restart ({statement}): {error}")
                });
        }
        let catalog = cassie
            .execute_sql(
                &session,
                "SELECT c.oid::BIGINT, a.atttypid::BIGINT, a.atttypmod::BIGINT \
                 FROM pg_catalog.pg_class AS c \
                 JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
                 JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid \
                 WHERE n.nspname = 'compat_issue839_restart' \
                   AND c.relname = 'rows_v2' AND a.attname = 'amount'",
                vec![],
            )
            .expect("read renamed relation and numeric column metadata before restart")
            .rows()
            .to_vec();
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT id::TEXT, amount::TEXT, note \
                 FROM compat_issue839_restart.rows_v2 ORDER BY id",
                vec![],
            )
            .expect("read converted rows before restart")
            .rows()
            .to_vec();
        drop(session);
        drop(cassie);
        (catalog, rows)
    };

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local Cassie instance");
    reopened
        .startup()
        .expect("recover renamed relation after restart");
    let session = reopened.create_session("tester", None);
    let catalog_after_restart = reopened
        .execute_sql(
            &session,
            "SELECT c.oid::BIGINT, a.atttypid::BIGINT, a.atttypmod::BIGINT \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid \
             WHERE n.nspname = 'compat_issue839_restart' \
               AND c.relname = 'rows_v2' AND a.attname = 'amount'",
            vec![],
        )
        .expect("read renamed relation and numeric column metadata after restart")
        .rows()
        .to_vec();
    let rows_after_restart = reopened
        .execute_sql(
            &session,
            "SELECT id::TEXT, amount::TEXT, note \
             FROM compat_issue839_restart.rows_v2 ORDER BY id",
            vec![],
        )
        .expect("read converted rows after restart")
        .rows()
        .to_vec();

    // Assert
    assert_renamed_alter_restart(
        &before_restart.0,
        &catalog_after_restart,
        &before_restart.1,
        &rows_after_restart,
    );
}
