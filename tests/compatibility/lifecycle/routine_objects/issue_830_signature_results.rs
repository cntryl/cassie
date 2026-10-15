use super::*;

#[test]
fn should_resolve_schema_qualified_builtin_calls() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue830_qualified_builtin_call").await;
        let (client, connection) = server.connect().await;

        // Act
        let row = client
            .query_one("SELECT pg_catalog.lower('MiXeD')", &[])
            .await
            .expect("resolve the pg_catalog-qualified lower routine");

        // Assert
        assert_eq!(row.columns()[0].type_().oid(), 25);
        assert_eq!(row.get::<_, String>(0), "mixed");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_resolve_schema_qualified_now_builtin() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue830_qualified_now_builtin").await;
        let (client, connection) = server.connect().await;

        // Act
        let row = client
            .query_one("SELECT pg_catalog.now() IS NOT NULL", &[])
            .await
            .expect("resolve the schema-qualified stable now routine");

        // Assert
        assert_eq!(row.columns()[0].type_().oid(), 16);
        assert!(row.get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_accept_signature_qualified_alter_function_cost() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue830_alter_function_cost").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("BEGIN")
            .await
            .expect("start a transaction for the routine catalog change");

        // Act
        client
            .batch_execute("ALTER FUNCTION pg_catalog.lower(text) COST 42")
            .await
            .expect("alter the selected overload's routine cost");
        let row = client
            .query_one("SELECT pg_catalog.lower('MiXeD')", &[])
            .await
            .expect("call the routine after changing its cost attribute");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the catalog change");

        // Assert
        assert_eq!(row.columns()[0].type_().oid(), 25);
        assert_eq!(row.get::<_, String>(0), "mixed");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_resolve_schema_qualified_routine_overloads_by_input_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue830_routine_overload_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_issue830_signatures",
                "CREATE FUNCTION compat_issue830_signatures.pick(input_value INTEGER) \
                   RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ \
                   SELECT 'int4:' || input_value::TEXT $$",
                "CREATE FUNCTION compat_issue830_signatures.pick(input_value BIGINT) \
                   RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ \
                   SELECT 'int8:' || input_value::TEXT $$",
            ],
        )
        .await
        .expect("create schema-qualified overloads with distinct input identities");

        // Act
        let row = client
            .query_one(
                "SELECT compat_issue830_signatures.pick(7::INTEGER), \
                        compat_issue830_signatures.pick(7::BIGINT)",
                &[],
            )
            .await
            .expect("resolve each schema-qualified call by its declared argument type");

        // Assert
        assert_eq!(row.columns()[0].type_().oid(), 25);
        assert_eq!(row.columns()[1].type_().oid(), 25);
        assert_eq!(row.get::<_, String>(0), "int4:7");
        assert_eq!(row.get::<_, String>(1), "int8:7");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_named_argument_call_notation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue830_routine_argument_notation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE SCHEMA compat_issue830_argument_notation; \
                 CREATE FUNCTION compat_issue830_argument_notation.joined( \
                   left_value TEXT, right_value TEXT DEFAULT 'fallback' \
                 ) RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ \
                   SELECT left_value || ':' || right_value $$",
            )
            .await
            .expect("create a SQL routine with a defaulted named argument");

        // Act
        let row = client
            .query_one(
                "SELECT compat_issue830_argument_notation.joined( \
                          'positional', right_value => 'mixed'), \
                        compat_issue830_argument_notation.joined( \
                          right_value := 'legacy', left_value := 'named'), \
                        compat_issue830_argument_notation.joined('defaulted')",
                &[],
            )
            .await
            .expect("call with mixed notation, legacy notation and a defaulted argument");
        let invalid_order = client
            .query(
                "SELECT compat_issue830_argument_notation.joined( \
                          right_value => 'named', 'late positional')",
                &[],
            )
            .await
            .expect_err("positional arguments cannot follow named arguments");
        let recovered: i32 = client
            .query_one("SELECT 1", &[])
            .await
            .expect("connection should recover after invalid argument notation")
            .get(0);

        // Assert
        assert_eq!(row.columns()[0].type_().oid(), 25);
        assert_eq!(row.columns()[1].type_().oid(), 25);
        assert_eq!(row.columns()[2].type_().oid(), 25);
        assert_eq!(row.get::<_, String>(0), "positional:mixed");
        assert_eq!(row.get::<_, String>(1), "named:legacy");
        assert_eq!(row.get::<_, String>(2), "defaulted:fallback");
        assert_eq!(db_error(&invalid_order).code().code(), "42601");
        assert_eq!(recovered, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_return_named_composite_values_with_declared_type_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue830_routine_result_descriptors").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_issue830_results",
                "CREATE TYPE compat_issue830_results.payload AS (item_count BIGINT, label TEXT)",
                "CREATE FUNCTION compat_issue830_results.return_payload( \
                   input_count BIGINT, input_label TEXT \
                 ) RETURNS compat_issue830_results.payload LANGUAGE SQL IMMUTABLE AS $$ \
                   SELECT ROW(input_count, input_label)::compat_issue830_results.payload $$",
            ],
        )
        .await
        .expect("create a function that returns a named composite type");

        // Act
        let composite = client
            .query_one(
                "SELECT compat_issue830_results.return_payload(42, 'Ada')",
                &[],
            )
            .await
            .expect("read the named-composite function result");
        let composite_oid: u32 = client
            .query_one(
                "SELECT 'compat_issue830_results.payload'::REGTYPE::OID",
                &[],
            )
            .await
            .expect("resolve the declared composite type OID")
            .get(0);
        let composite_text: String = client
            .query_one(
                "SELECT compat_issue830_results.return_payload(42, 'Ada')::TEXT",
                &[],
            )
            .await
            .expect("read the composite output in PostgreSQL text form")
            .get(0);
        // Assert
        assert_eq!(composite.columns()[0].type_().oid(), composite_oid);
        assert_eq!(composite_text, "(42,Ada)");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_describe_set_returning_table_columns_with_postgresql_types() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue830_table_result_descriptors").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE SCHEMA compat_issue830_table_results; \
                 CREATE FUNCTION compat_issue830_table_results.rows( \
                   include_rows BOOLEAN \
                 ) RETURNS TABLE(sequence BIGINT, note TEXT) \
                   LANGUAGE SQL IMMUTABLE AS $$ \
                   SELECT 2147483648::BIGINT, NULL::TEXT WHERE include_rows \
                   UNION ALL \
                   SELECT 9223372036854775807::BIGINT, 'edge'::TEXT \
                     WHERE include_rows $$",
            )
            .await
            .expect("create a multi-column set-returning function");

        // Act
        let rows = client
            .query(
                "SELECT * FROM compat_issue830_table_results.rows(TRUE) \
                 ORDER BY sequence",
                &[],
            )
            .await
            .expect("read all rows from the declared TABLE result");
        let empty_statement = client
            .prepare("SELECT * FROM compat_issue830_table_results.rows(FALSE)")
            .await
            .expect("describe the empty TABLE result");
        let empty_rows = client
            .query(&empty_statement, &[])
            .await
            .expect("execute the empty set-returning result");

        // Assert
        assert_eq!(rows[0].columns()[0].type_().oid(), 20);
        assert_eq!(rows[0].columns()[1].type_().oid(), 25);
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i64>(0), row.get::<_, Option<String>>(1)))
                .collect::<Vec<_>>(),
            vec![(2_147_483_648, None), (i64::MAX, Some("edge".to_owned()))]
        );
        assert_eq!(empty_statement.columns()[0].type_().oid(), 20);
        assert_eq!(empty_statement.columns()[1].type_().oid(), 25);
        assert!(empty_rows.is_empty());

        drop(client);
        server.shutdown(connection).await;
    });
}
