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
