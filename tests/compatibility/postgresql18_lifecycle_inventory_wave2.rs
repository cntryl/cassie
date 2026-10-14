use super::*;

fn lifecycle_wave_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread Tokio runtime")
}

#[test]
fn should_revoke_sequence_usage_without_advancing_the_sequence() {
    // Arrange
    let runtime = lifecycle_wave_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_usage_revocation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE ROLE compat_wave2_sequence_reader; \
                 CREATE SEQUENCE compat_wave2_sequence; \
                 GRANT USAGE ON SEQUENCE compat_wave2_sequence \
                   TO compat_wave2_sequence_reader",
            )
            .await
            .expect("create a sequence reader and grant USAGE");
        client
            .batch_execute("SET ROLE compat_wave2_sequence_reader")
            .await
            .expect("assume the sequence reader role");
        let first = client
            .query_one("SELECT nextval('compat_wave2_sequence')", &[])
            .await
            .expect("USAGE allows nextval");
        client
            .batch_execute(
                "RESET ROLE; REVOKE USAGE ON SEQUENCE compat_wave2_sequence \
                            FROM compat_wave2_sequence_reader; \
                            SET ROLE compat_wave2_sequence_reader",
            )
            .await
            .expect("revoke USAGE and re-assume the reader role");

        // Act
        let denied = client
            .query_one("SELECT nextval('compat_wave2_sequence')", &[])
            .await
            .expect_err("nextval requires sequence permission after USAGE is revoked");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role after the denied call");
        let sequence_state = client
            .query_one(
                "SELECT last_value, is_called FROM compat_wave2_sequence",
                &[],
            )
            .await
            .expect("read sequence state after the denied call");

        // Assert
        assert_eq!(first.get::<_, i64>(0), 1);
        assert_eq!(db_error(&denied).code().code(), "42501");
        assert_eq!(sequence_state.get::<_, i64>(0), 1);
        assert!(sequence_state.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_extension_dependencies_until_cascade_drop() {
    // Arrange
    let runtime = lifecycle_wave_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_dependency_drop").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE EXTENSION pgcrypto; \
                 CREATE VIEW compat_wave2_crypto_view AS \
                   SELECT digest('wave2', 'sha256') AS digest",
            )
            .await
            .expect("install pgcrypto and create a dependent view");

        // Act
        let restricted_drop = client
            .batch_execute("DROP EXTENSION pgcrypto")
            .await
            .expect_err("RESTRICT must reject dropping an extension with dependents");
        let retained = client
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM pg_catalog.pg_extension \
                    WHERE extname = 'pgcrypto'), \
                   (SELECT count(*) FROM pg_catalog.pg_class \
                    WHERE relname = 'compat_wave2_crypto_view')",
                &[],
            )
            .await
            .expect("verify RESTRICT left the extension and view intact");
        client
            .batch_execute("DROP EXTENSION pgcrypto CASCADE")
            .await
            .expect("CASCADE removes the extension and its dependent view");
        let removed = client
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM pg_catalog.pg_extension \
                    WHERE extname = 'pgcrypto'), \
                   (SELECT count(*) FROM pg_catalog.pg_class \
                    WHERE relname = 'compat_wave2_crypto_view')",
                &[],
            )
            .await
            .expect("verify the cascade removed both catalog entries");

        // Assert
        assert_eq!(db_error(&restricted_drop).code().code(), "2BP01");
        assert_eq!(retained.get::<_, i64>(0), 1);
        assert_eq!(retained.get::<_, i64>(1), 1);
        assert_eq!(removed.get::<_, i64>(0), 0);
        assert_eq!(removed.get::<_, i64>(1), 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_table_comment_at_each_rollback_scope() {
    // Arrange
    let runtime = lifecycle_wave_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("comment_transaction_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_wave2_comment_rows (id INT); \
                 COMMENT ON TABLE compat_wave2_comment_rows IS 'baseline'",
            )
            .await
            .expect("create a table with a baseline comment");

        // Act
        client
            .batch_execute(
                "BEGIN; \
                 COMMENT ON TABLE compat_wave2_comment_rows IS 'outer'; \
                 SAVEPOINT comment_inner; \
                 COMMENT ON TABLE compat_wave2_comment_rows IS 'inner'; \
                 ROLLBACK TO SAVEPOINT comment_inner",
            )
            .await
            .expect("roll back the inner comment update to its savepoint");
        let after_savepoint: Option<String> = client
            .query_one(
                "SELECT obj_description(\
                   'compat_wave2_comment_rows'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read the comment after savepoint rollback")
            .try_get(0)
            .expect("comment after savepoint rollback");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the outer comment update");
        let after_transaction: Option<String> = client
            .query_one(
                "SELECT obj_description(\
                   'compat_wave2_comment_rows'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read the comment after transaction rollback")
            .try_get(0)
            .expect("comment after transaction rollback");

        // Assert
        assert_eq!(after_savepoint.as_deref(), Some("outer"));
        assert_eq!(after_transaction.as_deref(), Some("baseline"));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_zero_fill_large_object_after_truncation_growth() {
    // Arrange
    let runtime = lifecycle_wave_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_truncate_growth").await;
        let (client, connection) = server.connect().await;
        let object_id: i32 = client
            .query_one(
                "SELECT lo_from_bytea(0, decode('6162636465', 'hex'))::int4",
                &[],
            )
            .await
            .expect("create an object containing five known bytes")
            .try_get(0)
            .expect("large-object OID as int4");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin large-object truncate transaction");
        let descriptor: i32 = client
            .query_one(&format!("SELECT lo_open({object_id}, 131072)"), &[])
            .await
            .expect("open the object for writing")
            .try_get(0)
            .expect("large-object descriptor");

        // Act
        let truncated: i32 = client
            .query_one(&format!("SELECT lo_truncate({descriptor}, 3)"), &[])
            .await
            .expect("truncate the object to three bytes")
            .try_get(0)
            .expect("truncate status");
        let shortened: String = client
            .query_one(
                &format!("SELECT encode(lo_get({object_id}::oid), 'hex')"),
                &[],
            )
            .await
            .expect("read the truncated object")
            .try_get(0)
            .expect("shortened object bytes");
        let extended: i32 = client
            .query_one(&format!("SELECT lo_truncate({descriptor}, 7)"), &[])
            .await
            .expect("extend the object to seven bytes")
            .try_get(0)
            .expect("extension status");
        let grown: String = client
            .query_one(
                &format!("SELECT encode(lo_get({object_id}::oid), 'hex')"),
                &[],
            )
            .await
            .expect("read the grown object")
            .try_get(0)
            .expect("grown object bytes");
        let closed: i32 = client
            .query_one(&format!("SELECT lo_close({descriptor})"), &[])
            .await
            .expect("close the large-object descriptor")
            .try_get(0)
            .expect("close status");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit large-object truncate transaction");

        // Assert
        assert_eq!(truncated, 0);
        assert_eq!(shortened, "616263");
        assert_eq!(extended, 0);
        assert_eq!(grown, "61626300000000");
        assert_eq!(closed, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
