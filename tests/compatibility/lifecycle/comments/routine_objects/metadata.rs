#[test]
fn should_roundtrip_sequence_comments_through_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_comment_sequence")
            .await
            .expect("create a sequence to annotate");

        // Act
        client
            .batch_execute("COMMENT ON SEQUENCE compat_comment_sequence IS 'sequence note'")
            .await
            .expect("store an object comment on the sequence");
        let comment: Option<String> = client
            .query_one(
                "SELECT obj_description('compat_comment_sequence'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read the sequence comment from PostgreSQL object metadata")
            .try_get(0)
            .expect("sequence comment");

        // Assert
        assert_eq!(comment.as_deref(), Some("sequence note"));

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_roundtrip_constraint_comments_through_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("constraint_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_constraint_comments ( \
                   id INT CONSTRAINT compat_constraint_comments_key UNIQUE)",
            )
            .await
            .expect("create a named constraint for comment metadata");

        // Act
        client
            .batch_execute(
                "COMMENT ON CONSTRAINT compat_constraint_comments_key \
                 ON compat_constraint_comments IS 'unique business key'",
            )
            .await
            .expect("set a comment on the named table constraint");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(oid, 'pg_constraint') \
                 FROM pg_constraint WHERE conname = 'compat_constraint_comments_key'",
                &[],
            )
            .await
            .expect("read the constraint comment")
            .try_get(0)
            .expect("comment value");
        client
            .batch_execute(
                "COMMENT ON CONSTRAINT compat_constraint_comments_key \
                 ON compat_constraint_comments IS NULL",
            )
            .await
            .expect("clear a constraint comment with IS NULL");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(oid, 'pg_constraint') \
                 FROM pg_constraint WHERE conname = 'compat_constraint_comments_key'",
                &[],
            )
            .await
            .expect("read the cleared constraint comment")
            .try_get(0)
            .expect("cleared comment value");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("unique business key"));
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_roundtrip_role_comments_through_shared_object_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("role_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE ROLE compat_comment_role NOLOGIN")
            .await
            .expect("create a role to annotate");

        // Act
        client
            .batch_execute("COMMENT ON ROLE compat_comment_role IS 'role metadata'")
            .await
            .expect("store a comment on the shared role object");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT shobj_description(oid, 'pg_authid') \
                 FROM pg_catalog.pg_authid WHERE rolname = 'compat_comment_role'",
                &[],
            )
            .await
            .expect("read the role comment from shared object metadata")
            .try_get(0)
            .expect("role comment");
        client
            .batch_execute("COMMENT ON ROLE compat_comment_role IS NULL")
            .await
            .expect("clear a role comment with IS NULL");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT shobj_description(oid, 'pg_authid') \
                 FROM pg_catalog.pg_authid WHERE rolname = 'compat_comment_role'",
                &[],
            )
            .await
            .expect("read the cleared role comment")
            .try_get(0)
            .expect("cleared role comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("role metadata"));
        assert_eq!(after_clear, None);

        client
            .batch_execute("DROP ROLE compat_comment_role")
            .await
            .expect("drop the annotated role");
        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_roundtrip_view_comment_through_relation_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("view_comments").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_view_comment_source (id INT)",
                "CREATE VIEW compat_comment_view AS \
                   SELECT id FROM compat_view_comment_source",
            ],
        )
        .await
        .expect("create a view to annotate");

        // Act
        let commented = client
            .batch_execute("COMMENT ON VIEW compat_comment_view IS 'view note'")
            .await;
        let comment = if commented.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT obj_description('compat_comment_view'::regclass, 'pg_class')",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        let comment_sqlstate = commented
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;

        assert!(
            commented.is_ok(),
            "COMMENT ON VIEW should store relation metadata; SQLSTATE: {comment_sqlstate:?}"
        );
        let comment = comment
            .expect("successful COMMENT ON VIEW should allow catalog inspection")
            .expect("read view comment");
        assert_eq!(
            comment.get::<_, Option<String>>(0).as_deref(),
            Some("view note")
        );
    });
}
#[test]
fn should_store_database_comments_in_shared_object_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("database_comments").await;
        let (client, connection) = server.connect().await;

        // Act
        let commented = client
            .batch_execute("COMMENT ON DATABASE postgres IS 'database note'")
            .await;
        let comment = if commented.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT shobj_description(oid, 'pg_database') \
                         FROM pg_catalog.pg_database WHERE datname = 'postgres'",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        let comment_sqlstate = commented
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;

        assert!(
            commented.is_ok(),
            "COMMENT ON DATABASE should store shared metadata; SQLSTATE: {comment_sqlstate:?}"
        );
        let comment = comment
            .expect("successful COMMENT ON DATABASE should allow catalog inspection")
            .expect("read database comment");
        assert_eq!(
            comment.get::<_, Option<String>>(0).as_deref(),
            Some("database note")
        );
    });
}
