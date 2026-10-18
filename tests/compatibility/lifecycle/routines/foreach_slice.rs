use super::*;

#[test]
fn should_collect_matrix_rows_with_foreach_slice() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_foreach_slice_rows").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_foreach_slice_rows() RETURNS TEXT \
                 LANGUAGE plpgsql AS $function$ \
                 DECLARE \
                   row_values INTEGER[]; \
                   result TEXT := ''; \
                 BEGIN \
                   FOREACH row_values SLICE 1 \
                     IN ARRAY ARRAY[[10, 20], [30, 40]] LOOP \
                     IF result <> '' THEN \
                       result := result || ';'; \
                     END IF; \
                     result := result || row_values[1]::TEXT || ':' || row_values[2]::TEXT; \
                   END LOOP; \
                   RETURN result; \
                 END; \
                 $function$",
            )
            .await
            .expect("create a PL/pgSQL function that collects each matrix row");

        // Act
        let result: String = client
            .query_one("SELECT compat_foreach_slice_rows()", &[])
            .await
            .expect("iterate the two-dimensional array by one-dimensional slices")
            .try_get(0)
            .expect("serialized matrix rows");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(result, "10:20;30:40");
    });
}
