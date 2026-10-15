use super::*;

#[test]
fn should_refresh_tsvector_from_source_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_tsvector_update_columns").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_tsvector_trigger_docs ( \
                   id INT PRIMARY KEY, title TEXT, body TEXT, search_vector TSVECTOR)",
                "CREATE TRIGGER compat_tsvector_trigger_refresh \
                   BEFORE INSERT OR UPDATE ON compat_tsvector_trigger_docs FOR EACH ROW \
                   EXECUTE FUNCTION pg_catalog.tsvector_update_trigger( \
                     search_vector, 'pg_catalog.simple', title, body)",
                "INSERT INTO compat_tsvector_trigger_docs \
                   VALUES (1, 'Ada rocks', 'SQL works', NULL)",
            ],
        )
        .await
        .expect("create a built-in trigger that derives a tsvector from source columns");

        // Act
        let inserted = client
            .query_one(
                "SELECT search_vector::TEXT FROM compat_tsvector_trigger_docs WHERE id = 1",
                &[],
            )
            .await
            .expect("read the tsvector generated before insertion");
        client
            .batch_execute(
                "UPDATE compat_tsvector_trigger_docs SET title = 'Ada builds' WHERE id = 1",
            )
            .await
            .expect("refresh the tsvector after a source-column update");
        let updated = client
            .query_one(
                "SELECT search_vector::TEXT FROM compat_tsvector_trigger_docs WHERE id = 1",
                &[],
            )
            .await
            .expect("read the tsvector generated before the updated row is stored");

        // Assert
        assert_eq!(
            inserted.get::<_, String>(0),
            "'ada':1 'rocks':2 'sql':3 'works':4"
        );
        assert_eq!(
            updated.get::<_, String>(0),
            "'ada':1 'builds':2 'sql':3 'works':4"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
