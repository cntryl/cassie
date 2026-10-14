use super::*;

#[test]
fn should_bind_null_value_with_dynamic_execute_using() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_dynamic_execute_using_null").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_do_using_null (id INT PRIMARY KEY, payload TEXT)")
            .await
            .expect("create a target with a nullable text column");

        // Act
        client
            .batch_execute(
                r#"DO $block$
                   DECLARE optional_payload TEXT := NULL;
                   BEGIN
                     EXECUTE 'INSERT INTO compat_do_using_null (id, payload) VALUES ($1, $2)'
                       USING 42, optional_payload;
                   END;
                   $block$"#,
            )
            .await
            .expect("bind a NULL PL/pgSQL variable as a dynamic SQL parameter");
        let row = client
            .query_one(
                "SELECT id, payload, payload IS NULL FROM compat_do_using_null",
                &[],
            )
            .await
            .expect("read the value inserted through dynamic EXECUTE USING");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(row.get::<_, i32>(0), 42);
        assert_eq!(row.get::<_, Option<String>>(1), None);
        assert!(row.get::<_, bool>(2));
    });
}
