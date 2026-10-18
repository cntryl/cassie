use super::*;

#[test]
fn should_use_each_rows_text_search_configuration() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("tsvector_update_config_column").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_tsvector_config_docs ( \
                   id INT PRIMARY KEY, config REGCONFIG NOT NULL, title TEXT, body TEXT, \
                   search_vector TSVECTOR)",
                "CREATE TRIGGER compat_tsvector_config_refresh \
                   BEFORE INSERT OR UPDATE ON compat_tsvector_config_docs FOR EACH ROW \
                   EXECUTE FUNCTION pg_catalog.tsvector_update_trigger_column( \
                     search_vector, config, title, body)",
                "INSERT INTO compat_tsvector_config_docs VALUES \
                   (1, 'pg_catalog.simple'::REGCONFIG, 'The Fat Rat', 'Dogs Run', NULL), \
                   (2, 'pg_catalog.english'::REGCONFIG, 'The Fat Rat', 'Dogs Run', NULL)",
            ],
        )
        .await
        .expect("create a built-in trigger using each row's regconfig column");

        // Act
        client
            .batch_execute(
                "UPDATE compat_tsvector_config_docs SET title = 'The Fat Cats' WHERE id = 2",
            )
            .await
            .expect("refresh one row using its English configuration");
        let vectors = client
            .query(
                "SELECT id, search_vector::TEXT \
                 FROM compat_tsvector_config_docs ORDER BY id",
                &[],
            )
            .await
            .expect("read vectors generated with row-specific configurations");

        // Assert
        assert_eq!(vectors.len(), 2);
        assert_eq!(vectors[0].get::<_, i32>(0), 1);
        assert_eq!(
            vectors[0].get::<_, String>(1),
            "'dogs':4 'fat':2 'rat':3 'run':5 'the':1"
        );
        assert_eq!(vectors[1].get::<_, i32>(0), 2);
        assert_eq!(
            vectors[1].get::<_, String>(1),
            "'cat':3 'dog':4 'fat':2 'run':5"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
