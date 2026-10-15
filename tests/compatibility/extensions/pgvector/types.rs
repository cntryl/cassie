use super::*;

#[test]
fn should_describe_vector_type_given_a_bare_vector_cast_when_dimension_is_inferred() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let (server, client, connection, vector_oid) =
            start_vector_server("pgvector_type_descriptor").await;
        let statement = client
            .prepare("SELECT '[1,2,3]'::vector")
            .await
            .expect("prepare a bare vector result");

        // Act
        let value = client
            .query_one(
                "SELECT vector_dims('[1,2,3]'::vector), \
                 '[1,2,3]'::vector::text, pg_typeof('[1,2,3]'::vector)::text",
                &[],
            )
            .await
            .expect("read vector dimension, text value, and resolved type");

        // Assert
        assert_eq!(
            statement.columns()[0].type_().oid(),
            u32::try_from(vector_oid).expect("catalog OID should be nonnegative")
        );
        assert_eq!(value.get::<_, i32>(0), 3);
        assert_eq!(value.get::<_, String>(1), "[1,2,3]");
        assert_eq!(value.get::<_, String>(2), "vector");
        drop(client);
        server.shutdown(connection).await;
    });
}
