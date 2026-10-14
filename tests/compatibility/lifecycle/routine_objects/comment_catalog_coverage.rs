use super::*;

mod index_comment_clear;
mod null_clear_residuals;

#[test]
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

#[test]
fn should_resolve_overloaded_function_comments_by_identity_arguments() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("function_comment_signatures").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE FUNCTION compat_comment_function(input_value INT) \
                   RETURNS INT LANGUAGE SQL IMMUTABLE AS $$ SELECT 1 $$",
                "CREATE FUNCTION compat_comment_function(input_value TEXT) \
                   RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT 'text' $$",
            ],
        )
        .await
        .expect("create overloaded functions with distinct identity arguments");

        // Act
        execute_statements(
            &client,
            &[
                "COMMENT ON FUNCTION compat_comment_function(integer) \
                   IS 'integer implementation'",
                "COMMENT ON FUNCTION compat_comment_function(text) \
                   IS 'text implementation'",
            ],
        )
        .await
        .expect("comment each function overload by its identity arguments");
        let comments = client
            .query_one(
                "SELECT obj_description(\
                         'compat_comment_function(integer)'::regprocedure, 'pg_proc'), \
                        obj_description(\
                         'compat_comment_function(text)'::regprocedure, 'pg_proc')",
                &[],
            )
            .await
            .expect("read both overloaded function comments");
        client
            .batch_execute("COMMENT ON FUNCTION compat_comment_function(integer) IS NULL")
            .await
            .expect("clear only the integer overload comment");
        let cleared = client
            .query_one(
                "SELECT obj_description(\
                         'compat_comment_function(integer)'::regprocedure, 'pg_proc'), \
                        obj_description(\
                         'compat_comment_function(text)'::regprocedure, 'pg_proc')",
                &[],
            )
            .await
            .expect("read function comments after clearing one overload");

        // Assert
        assert_eq!(
            (
                comments.get::<_, Option<String>>(0).as_deref(),
                comments.get::<_, Option<String>>(1).as_deref(),
            ),
            (Some("integer implementation"), Some("text implementation"))
        );
        assert_eq!(cleared.get::<_, Option<String>>(0), None);
        assert_eq!(
            cleared.get::<_, Option<String>>(1).as_deref(),
            Some("text implementation")
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roundtrip_procedure_comments_by_identity_signature() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("procedure_comment_signature").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE PROCEDURE compat_comment_procedure(input_value INT) \
                   AS $$ SELECT 1 $$ LANGUAGE SQL",
            )
            .await
            .expect("create a procedure with an integer identity argument");

        // Act
        client
            .batch_execute(
                "COMMENT ON PROCEDURE compat_comment_procedure(integer) \
                   IS 'integer procedure'",
            )
            .await
            .expect("comment the procedure by its identity signature");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(\
                         'compat_comment_procedure(integer)'::regprocedure, 'pg_proc')",
                &[],
            )
            .await
            .expect("read the procedure comment")
            .try_get(0)
            .expect("procedure comment");
        client
            .batch_execute("COMMENT ON PROCEDURE compat_comment_procedure(integer) IS NULL")
            .await
            .expect("clear the procedure comment");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(\
                         'compat_comment_procedure(integer)'::regprocedure, 'pg_proc')",
                &[],
            )
            .await
            .expect("read the cleared procedure comment")
            .try_get(0)
            .expect("cleared procedure comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("integer procedure"));
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roundtrip_generic_routine_comments_by_identity_signature() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("routine_comment_signature").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_comment_routine(input_value BIGINT) \
                   RETURNS BIGINT LANGUAGE SQL IMMUTABLE AS $$ SELECT 1 $$",
            )
            .await
            .expect("create a function addressed through the ROUTINE alias");

        // Act
        client
            .batch_execute(
                "COMMENT ON ROUTINE compat_comment_routine(bigint) \
                   IS 'generic routine comment'",
            )
            .await
            .expect("comment the routine by its identity signature");
        let comment: Option<String> = client
            .query_one(
                "SELECT obj_description(\
                         'compat_comment_routine(bigint)'::regprocedure, 'pg_proc')",
                &[],
            )
            .await
            .expect("read the generic routine comment")
            .try_get(0)
            .expect("generic routine comment");

        // Assert
        assert_eq!(comment.as_deref(), Some("generic routine comment"));

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
fn should_roundtrip_trigger_comments_through_trigger_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_comments").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_comment_trigger_rows (id INT)",
                "CREATE FUNCTION compat_comment_trigger_fn() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$",
                "CREATE TRIGGER compat_comment_trigger BEFORE INSERT \
                   ON compat_comment_trigger_rows FOR EACH ROW \
                   EXECUTE FUNCTION compat_comment_trigger_fn()",
            ],
        )
        .await
        .expect("create a trigger object to annotate");

        // Act
        client
            .batch_execute(
                "COMMENT ON TRIGGER compat_comment_trigger \
                   ON compat_comment_trigger_rows IS 'row insertion hook'",
            )
            .await
            .expect("store a comment on the table trigger");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(trigger.oid, 'pg_trigger') \
                 FROM pg_catalog.pg_trigger AS trigger \
                 WHERE trigger.tgname = 'compat_comment_trigger' \
                   AND trigger.tgrelid = 'compat_comment_trigger_rows'::regclass",
                &[],
            )
            .await
            .expect("read the trigger comment from pg_trigger")
            .try_get(0)
            .expect("trigger comment");
        client
            .batch_execute(
                "COMMENT ON TRIGGER compat_comment_trigger \
                   ON compat_comment_trigger_rows IS NULL",
            )
            .await
            .expect("clear the trigger comment");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(trigger.oid, 'pg_trigger') \
                 FROM pg_catalog.pg_trigger AS trigger \
                 WHERE trigger.tgname = 'compat_comment_trigger' \
                   AND trigger.tgrelid = 'compat_comment_trigger_rows'::regclass",
                &[],
            )
            .await
            .expect("read the cleared trigger comment")
            .try_get(0)
            .expect("cleared trigger comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("row insertion hook"));
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
fn should_roundtrip_text_search_dictionary_comments_through_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("text_search_dictionary_comments").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute(
                "COMMENT ON TEXT SEARCH DICTIONARY pg_catalog.simple \
                   IS 'simple dictionary metadata'",
            )
            .await
            .expect("store a comment on the built-in simple dictionary");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(dictionary.oid, 'pg_ts_dict') \
                 FROM pg_catalog.pg_ts_dict AS dictionary \
                 WHERE dictionary.dictname = 'simple' \
                   AND dictionary.dictnamespace = 'pg_catalog'::regnamespace",
                &[],
            )
            .await
            .expect("read the dictionary comment from pg_ts_dict")
            .try_get(0)
            .expect("dictionary comment");
        client
            .batch_execute("COMMENT ON TEXT SEARCH DICTIONARY pg_catalog.simple IS NULL")
            .await
            .expect("clear the text-search dictionary comment");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(dictionary.oid, 'pg_ts_dict') \
                 FROM pg_catalog.pg_ts_dict AS dictionary \
                 WHERE dictionary.dictname = 'simple' \
                   AND dictionary.dictnamespace = 'pg_catalog'::regnamespace",
                &[],
            )
            .await
            .expect("read the cleared dictionary comment")
            .try_get(0)
            .expect("cleared dictionary comment");

        // Assert
        assert_eq!(before_clear.as_deref(), Some("simple dictionary metadata"));
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roundtrip_text_search_configuration_comments_through_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("text_search_configuration_comments").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute(
                "COMMENT ON TEXT SEARCH CONFIGURATION pg_catalog.simple \
                   IS 'simple configuration metadata'",
            )
            .await
            .expect("store a comment on the built-in simple configuration");
        let before_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(configuration.oid, 'pg_ts_config') \
                 FROM pg_catalog.pg_ts_config AS configuration \
                 WHERE configuration.cfgname = 'simple' \
                   AND configuration.cfgnamespace = 'pg_catalog'::regnamespace",
                &[],
            )
            .await
            .expect("read the configuration comment from pg_ts_config")
            .try_get(0)
            .expect("configuration comment");
        client
            .batch_execute("COMMENT ON TEXT SEARCH CONFIGURATION pg_catalog.simple IS NULL")
            .await
            .expect("clear the text-search configuration comment");
        let after_clear: Option<String> = client
            .query_one(
                "SELECT obj_description(configuration.oid, 'pg_ts_config') \
                 FROM pg_catalog.pg_ts_config AS configuration \
                 WHERE configuration.cfgname = 'simple' \
                   AND configuration.cfgnamespace = 'pg_catalog'::regnamespace",
                &[],
            )
            .await
            .expect("read the cleared configuration comment")
            .try_get(0)
            .expect("cleared configuration comment");

        // Assert
        assert_eq!(
            before_clear.as_deref(),
            Some("simple configuration metadata")
        );
        assert_eq!(after_clear, None);

        drop(client);
        server.shutdown(connection).await;
    });
}
