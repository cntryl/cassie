use super::*;

#[path = "index_comment_clear.rs"]
mod index_comment_clear;
#[path = "null_clear_residuals.rs"]
mod null_clear_residuals;

#[test]
fn should_reject_materialized_view_comments_until_metadata_support_is_implemented() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE MATERIALIZED VIEW compat_comment_mv AS SELECT 1 AS id")
            .await
            .expect("create a materialized view to test comment rejection");

        // Act
        let result = client
            .batch_execute("COMMENT ON MATERIALIZED VIEW compat_comment_mv IS 'cached rows'")
            .await;
        let error = result.expect_err("materialized-view comments remain pending under #845");

        // Assert
        assert_eq!(db_error(&error).code().code(), "0A000");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_roundtrip_domain_comments_through_type_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("domain_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE DOMAIN compat_comment_domain AS INT \
                   CONSTRAINT compat_comment_domain_positive CHECK (VALUE > 0)",
            )
            .await
            .expect("create a domain to annotate");

        // Act
        client
            .batch_execute("COMMENT ON DOMAIN compat_comment_domain IS 'positive integer'")
            .await
            .expect("store a comment on the domain");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description('compat_comment_domain'::regtype, 'pg_type')",
                &[],
            )
            .await
            .expect("read the domain comment")
            .try_get(0)
            .expect("domain comment");
        client
            .batch_execute("COMMENT ON DOMAIN compat_comment_domain IS NULL")
            .await
            .expect("clear the domain comment");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description('compat_comment_domain'::regtype, 'pg_type')",
                &[],
            )
            .await
            .expect("read the cleared domain comment")
            .try_get(0)
            .expect("cleared domain comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("positive integer"));
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_roundtrip_domain_constraint_comments_through_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("domain_constraint_comments").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE DOMAIN compat_comment_positive AS INT \
                   CONSTRAINT compat_comment_positive_check CHECK (VALUE > 0)",
            )
            .await
            .expect("create a domain with a named constraint");

        // Act
        client
            .batch_execute(
                "COMMENT ON CONSTRAINT compat_comment_positive_check \
                   ON DOMAIN compat_comment_positive IS 'domain range requirement'",
            )
            .await
            .expect("comment on a domain constraint");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(oid, 'pg_constraint') \
                 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_comment_positive_check'",
                &[],
            )
            .await
            .expect("read the domain-constraint comment")
            .try_get(0)
            .expect("domain-constraint comment");
        client
            .batch_execute(
                "COMMENT ON CONSTRAINT compat_comment_positive_check \
                   ON DOMAIN compat_comment_positive IS NULL",
            )
            .await
            .expect("clear the domain-constraint comment");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(oid, 'pg_constraint') \
                 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_comment_positive_check'",
                &[],
            )
            .await
            .expect("read the cleared domain-constraint comment")
            .try_get(0)
            .expect("cleared domain-constraint comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("domain range requirement"));
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_roundtrip_policy_comments_through_policy_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("policy_comments").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_comment_policy_rows (id INT)",
                "ALTER TABLE compat_comment_policy_rows ENABLE ROW LEVEL SECURITY",
                "CREATE POLICY compat_comment_policy ON compat_comment_policy_rows \
                   USING (id > 0)",
            ],
        )
        .await
        .expect("create a row-level security policy to annotate");

        // Act
        client
            .batch_execute(
                "COMMENT ON POLICY compat_comment_policy \
                   ON compat_comment_policy_rows IS 'positive identifiers only'",
            )
            .await
            .expect("store a comment on the row-level security policy");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(policy.oid, 'pg_policy') \
                 FROM pg_catalog.pg_policy AS policy \
                 WHERE policy.polname = 'compat_comment_policy' \
                   AND policy.polrelid = 'compat_comment_policy_rows'::regclass",
                &[],
            )
            .await
            .expect("read the policy comment from pg_policy")
            .try_get(0)
            .expect("policy comment");
        client
            .batch_execute(
                "COMMENT ON POLICY compat_comment_policy \
                   ON compat_comment_policy_rows IS NULL",
            )
            .await
            .expect("clear the policy comment");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(policy.oid, 'pg_policy') \
                 FROM pg_catalog.pg_policy AS policy \
                 WHERE policy.polname = 'compat_comment_policy' \
                   AND policy.polrelid = 'compat_comment_policy_rows'::regclass",
                &[],
            )
            .await
            .expect("read the cleared policy comment")
            .try_get(0)
            .expect("cleared policy comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("positive identifiers only"));
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_roundtrip_extension_comments_through_extension_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("extension_comments").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("COMMENT ON EXTENSION plpgsql IS 'procedural language runtime'")
            .await
            .expect("store a comment on the installed plpgsql extension");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(extension.oid, 'pg_extension') \
                 FROM pg_catalog.pg_extension AS extension \
                 WHERE extension.extname = 'plpgsql'",
                &[],
            )
            .await
            .expect("read the extension comment from pg_extension")
            .try_get(0)
            .expect("extension comment");
        client
            .batch_execute("COMMENT ON EXTENSION plpgsql IS NULL")
            .await
            .expect("clear the extension comment");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(extension.oid, 'pg_extension') \
                 FROM pg_catalog.pg_extension AS extension \
                 WHERE extension.extname = 'plpgsql'",
                &[],
            )
            .await
            .expect("read the cleared extension comment")
            .try_get(0)
            .expect("cleared extension comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("procedural language runtime"));
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
#[ignore = "Pending #826 and #845 materialized-view comment metadata; #845 depends on #817"]
fn should_roundtrip_materialized_view_comments_through_relation_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_comments").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_comment_mv_source (id INT)",
                "INSERT INTO compat_comment_mv_source VALUES (1), (2)",
                "CREATE MATERIALIZED VIEW compat_comment_mv AS \
                   SELECT id FROM compat_comment_mv_source",
            ],
        )
        .await
        .expect("create a materialized view to annotate");

        // Act
        client
            .batch_execute("COMMENT ON MATERIALIZED VIEW compat_comment_mv IS 'cached rows'")
            .await
            .expect("store a comment on the materialized view");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description('compat_comment_mv'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read the materialized-view comment")
            .try_get(0)
            .expect("materialized-view comment");
        client
            .batch_execute("COMMENT ON MATERIALIZED VIEW compat_comment_mv IS NULL")
            .await
            .expect("clear the materialized-view comment");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description('compat_comment_mv'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read the cleared materialized-view comment")
            .try_get(0)
            .expect("cleared materialized-view comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("cached rows"));
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}
