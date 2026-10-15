use super::*;

#[test]
fn should_preserve_enum_support_function_result_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_support_function_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_enum_function_identity",
                "CREATE TYPE compat_enum_function_identity.state_code \
                   AS ENUM ('open', 'ready', 'closed')",
            ],
        )
        .await
        .expect("create an enum type for support-function output descriptors");
        let type_oids = client
            .query_one(
                "SELECT oid::BIGINT, typarray::BIGINT FROM pg_type \
                 WHERE oid = 'compat_enum_function_identity.state_code'::REGTYPE",
                &[],
            )
            .await
            .expect("read the enum and generated array OIDs");
        let enum_oid = type_oids.get::<_, i64>(0);
        let enum_array_oid = type_oids.get::<_, i64>(1);

        // Act
        let results = client
            .query_one(
                "SELECT enum_first(NULL::compat_enum_function_identity.state_code), \
                        enum_last(NULL::compat_enum_function_identity.state_code), \
                        enum_range(NULL::compat_enum_function_identity.state_code), \
                        enum_range('open'::compat_enum_function_identity.state_code, \
                                   'ready'::compat_enum_function_identity.state_code)",
                &[],
            )
            .await
            .expect("read native enum support-function results");
        let values = client
            .query_one(
                "SELECT enum_first(NULL::compat_enum_function_identity.state_code)::TEXT, \
                        enum_last(NULL::compat_enum_function_identity.state_code)::TEXT, \
                        enum_range(NULL::compat_enum_function_identity.state_code)::TEXT, \
                        enum_range('open'::compat_enum_function_identity.state_code, \
                                   'ready'::compat_enum_function_identity.state_code)::TEXT",
                &[],
            )
            .await
            .expect("read the labels produced by enum support functions");

        // Assert
        assert_eq!(
            results
                .columns()
                .iter()
                .map(|column| i64::from(column.type_().oid()))
                .collect::<Vec<_>>(),
            vec![enum_oid, enum_oid, enum_array_oid, enum_array_oid]
        );
        assert_eq!(results.columns()[0].type_().name(), "state_code");
        assert_eq!(results.columns()[1].type_().name(), "state_code");
        assert_eq!(results.columns()[2].type_().name(), "_state_code");
        assert_eq!(results.columns()[3].type_().name(), "_state_code");
        assert_eq!(values.get::<_, String>(0), "open");
        assert_eq!(values.get::<_, String>(1), "closed");
        assert_eq!(values.get::<_, String>(2), "{open,ready,closed}");
        assert_eq!(values.get::<_, String>(3), "{open,ready}");

        drop(client);
        server.shutdown(connection).await;
    });
}
