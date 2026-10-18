use super::*;

#[test]
fn should_call_a_sql_language_function() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sql_language_function").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_issue_830_language_add_one(input_value INTEGER) \
                   RETURNS INTEGER LANGUAGE SQL AS \
                   'SELECT input_value + 1'",
            )
            .await
            .expect("create scalar function using the SQL language clause");

        // Act
        let result: i32 = client
            .query_one("SELECT compat_issue_830_language_add_one(41)", &[])
            .await
            .expect("call the SQL-language function")
            .try_get(0)
            .expect("function result");

        // Assert
        assert_eq!(result, 42);

        drop(client);
        server.shutdown(connection).await;
    });
}
