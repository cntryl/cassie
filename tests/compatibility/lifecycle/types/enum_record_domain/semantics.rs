#[test]
fn should_preserve_enum_result_type_identities() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_schema_array_identity_wave6").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_enum_identity_wave6",
                "CREATE TYPE compat_enum_identity_wave6.state_code \
                   AS ENUM ('open', 'ready', 'closed')",
            ],
        )
        .await
        .expect("create a schema-scoped enum for bound scalar and array results");

        // Act
        let scalar = client
            .query_one(
                "SELECT $1::compat_enum_identity_wave6.state_code AS state",
                &[&"ready"],
            )
            .await
            .expect("cast a bound string to the named enum type");
        let comparison = client
            .query_one(
                "SELECT $1::compat_enum_identity_wave6.state_code \
                   > 'open'::compat_enum_identity_wave6.state_code",
                &[&"ready"],
            )
            .await
            .expect("compare enum values by declaration order");
        let array = client
            .query_one(
                "SELECT ARRAY[$1::compat_enum_identity_wave6.state_code, \
                              NULL::compat_enum_identity_wave6.state_code] AS states",
                &[&"ready"],
            )
            .await
            .expect("construct an array retaining the enum element type");
        let array_text: String = client
            .query_one(
                "SELECT ARRAY[$1::compat_enum_identity_wave6.state_code, \
                              NULL::compat_enum_identity_wave6.state_code]::TEXT",
                &[&"ready"],
            )
            .await
            .expect("render enum array values including NULL")
            .get(0);

        // Assert
        assert_eq!(scalar.columns()[0].type_().name(), "state_code");
        assert_eq!(
            scalar.columns()[0].type_().schema(),
            "compat_enum_identity_wave6"
        );
        assert!(comparison.get::<_, bool>(0));
        assert_eq!(array.columns()[0].type_().name(), "_state_code");
        assert_eq!(
            array.columns()[0].type_().schema(),
            "compat_enum_identity_wave6"
        );
        assert_eq!(array_text, "{ready,NULL}");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_expand_parenthesized_relation_rows_into_named_fields() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("parenthesized_row_expansion_wave6").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_row_expansion_wave6 (id INT, label TEXT)",
                "INSERT INTO compat_row_expansion_wave6 VALUES (7, 'seven')",
            ],
        )
        .await
        .expect("create a relation for whole-row field expansion");

        // Act
        let rows = client
            .query(
                "SELECT (compat_row_expansion_wave6).* \
                 FROM compat_row_expansion_wave6",
                &[],
            )
            .await
            .expect("expand a parenthesized whole-row expression into its fields");

        // Assert
        assert_eq!(
            rows[0]
                .columns()
                .iter()
                .map(tokio_postgres::Column::name)
                .collect::<Vec<_>>(),
            ["id", "label"]
        );
        assert_eq!(rows[0].get::<_, i32>(0), 7);
        assert_eq!(rows[0].get::<_, String>(1), "seven");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_resolve_composite_fields_with_function_notation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_function_notation_wave6").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TYPE compat_row_notation_wave6 AS (id INT, label TEXT)")
            .await
            .expect("create a named composite for field function notation");

        // Act
        let row = client
            .query_one(
                "SELECT id(ROW(7, 'seven')::compat_row_notation_wave6), \
                        label(ROW(7, 'seven')::compat_row_notation_wave6)",
                &[],
            )
            .await
            .expect("resolve field-name function notation on a composite value");

        // Assert
        assert_eq!(row.get::<_, i32>(0), 7);
        assert_eq!(row.get::<_, String>(1), "seven");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_aggregate_named_composite_values_lexicographically() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_min_max_wave6").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_composite_min_max_wave6 AS (id INT, label TEXT)",
                "CREATE TABLE compat_composite_min_max_rows_wave6 \
                 (value compat_composite_min_max_wave6)",
                "INSERT INTO compat_composite_min_max_rows_wave6 VALUES \
                 (ROW(2, 'b')::compat_composite_min_max_wave6), \
                 (ROW(1, 'c')::compat_composite_min_max_wave6), \
                 (ROW(1, 'a')::compat_composite_min_max_wave6)",
            ],
        )
        .await
        .expect("create composite row values with distinct lexicographic extrema");

        // Act
        let row = client
            .query_one(
                "SELECT min(value)::TEXT, max(value)::TEXT \
                 FROM compat_composite_min_max_rows_wave6",
                &[],
            )
            .await
            .expect("aggregate composite row values with PostgreSQL min and max");

        // Assert
        assert_eq!(row.get::<_, String>(0), "(1,a)");
        assert_eq!(row.get::<_, String>(1), "(2,b)");

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_insert_values_into_named_composite_fields() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_field_insert_wave6").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_address_insert_wave6 AS \
                 (street TEXT, postal_code INT)",
                "CREATE TABLE compat_address_rows_wave6 \
                 (id INT, address compat_address_insert_wave6)",
            ],
        )
        .await
        .expect("create a table that stores a named composite value");

        // Act
        client
            .batch_execute(
                "INSERT INTO compat_address_rows_wave6 \
                 (id, address.street, address.postal_code) \
                 VALUES (1, 'Main', 12345)",
            )
            .await
            .expect("insert by naming individual composite fields");
        let row = client
            .query_one(
                "SELECT (address).street, (address).postal_code \
                 FROM compat_address_rows_wave6 WHERE id = 1",
                &[],
            )
            .await
            .expect("read fields populated by the composite-column insert");

        // Assert
        assert_eq!(row.get::<_, String>(0), "Main");
        assert_eq!(row.get::<_, i32>(1), 12345);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_enforce_not_valid_domain_constraints_through_validation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("domain_not_valid_wave6").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE DOMAIN compat_nonempty_wave6 AS TEXT",
                "CREATE TABLE compat_domain_rows_wave6 \
                 (id INT PRIMARY KEY, value compat_nonempty_wave6)",
                "INSERT INTO compat_domain_rows_wave6 VALUES (1, '')",
                "ALTER DOMAIN compat_nonempty_wave6 ADD CONSTRAINT \
                 compat_nonempty_check_wave6 CHECK (VALUE <> '') NOT VALID",
            ],
        )
        .await
        .expect("add a NOT VALID domain check while retaining existing violating rows");

        // Act
        client
            .execute(
                "INSERT INTO compat_domain_rows_wave6 VALUES (2, 'valid')",
                &[],
            )
            .await
            .expect("allow a valid value after adding a NOT VALID check");
        let invalid_insert = client
            .execute("INSERT INTO compat_domain_rows_wave6 VALUES (3, '')", &[])
            .await
            .expect_err("reject a new value that violates the domain check");
        let invalid_existing = client
            .batch_execute(
                "ALTER DOMAIN compat_nonempty_wave6 \
                 VALIDATE CONSTRAINT compat_nonempty_check_wave6",
            )
            .await
            .expect_err("reject validation while a preexisting row violates the check");
        client
            .execute("DELETE FROM compat_domain_rows_wave6 WHERE id = 1", &[])
            .await
            .expect("remove the preexisting violating value");
        client
            .batch_execute(
                "ALTER DOMAIN compat_nonempty_wave6 \
                 VALIDATE CONSTRAINT compat_nonempty_check_wave6",
            )
            .await
            .expect("validate the domain check after invalid existing data is removed");

        // Assert
        assert_eq!(db_error(&invalid_insert).code().code(), "23514");
        assert_eq!(db_error(&invalid_existing).code().code(), "23514");

        drop(client);
        server.shutdown(connection).await;
    });
}
