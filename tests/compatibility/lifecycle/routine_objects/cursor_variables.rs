use super::*;

// This is a #835 cursor acceptance scenario. At the current baseline, CREATE
// FUNCTION parses every body as a SQL expression, so the generic PL/pgSQL
// body prerequisite masks OPEN/FETCH/CLOSE execution and this is not direct
// cursor RED evidence yet.
#[test]
fn should_fetch_bound_plpgsql_cursor_value() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_bound_cursor").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_plpgsql_cursor_rows (id INT PRIMARY KEY, payload TEXT NOT NULL)",
                "INSERT INTO compat_plpgsql_cursor_rows VALUES (1, 'first'), (2, 'second')",
                "CREATE FUNCTION compat_plpgsql_fetch_bound_cursor(input_id INT) RETURNS TEXT \
                   LANGUAGE plpgsql AS $function$ \
                   DECLARE row_cursor CURSOR FOR \
                     SELECT payload FROM compat_plpgsql_cursor_rows WHERE id = input_id; \
                           fetched_value TEXT; \
                   BEGIN \
                     OPEN row_cursor; \
                     FETCH row_cursor INTO fetched_value; \
                     CLOSE row_cursor; \
                     RETURN fetched_value; \
                   END; \
                   $function$",
            ],
        )
        .await
        .expect("create and seed the bound-cursor PL/pgSQL fixture");

        // Act
        let fetched_value: String = client
            .query_one("SELECT compat_plpgsql_fetch_bound_cursor(2)", &[])
            .await
            .expect("execute OPEN, FETCH and CLOSE on a bound cursor variable")
            .try_get(0)
            .expect("fetched cursor value");

        // Assert
        assert_eq!(fetched_value, "second");

        drop(client);
        server.shutdown(connection).await;
    });
}
