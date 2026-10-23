use super::*;

async fn assert_unselected_index_is_rejected(
    client: &tokio_postgres::Client,
    statement: &str,
    index_name: &str,
) {
    let rejected = client
        .batch_execute(statement)
        .await
        .expect_err("unselected SQL index methods must be rejected");
    let index_count: i64 = client
        .query_one(
            &format!("SELECT count(*) FROM pg_catalog.pg_indexes WHERE indexname = '{index_name}'"),
            &[],
        )
        .await
        .expect("check that the rejected index was not published")
        .try_get(0)
        .expect("index count");

    assert_eq!(db_error(&rejected).code().code(), "0A000");
    assert_eq!(index_count, 0);
}

#[test]
fn should_reject_jsonb_gin_operator_classes_until_method_support_is_selected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("jsonb_gin_operator_classes").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_jsonb_index_rows (body JSONB)")
            .await
            .expect("create a JSONB table for the unsupported GIN method");

        // Act
        // Assert
        assert_unselected_index_is_rejected(
            &client,
            "CREATE INDEX compat_jsonb_ops_idx \
             ON compat_jsonb_index_rows USING gin (body jsonb_ops)",
            "compat_jsonb_ops_idx",
        )
        .await;
        assert_unselected_index_is_rejected(
            &client,
            "CREATE INDEX compat_jsonb_path_ops_idx \
             ON compat_jsonb_index_rows USING gin (body jsonb_path_ops)",
            "compat_jsonb_path_ops_idx",
        )
        .await;

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_sql_full_text_access_methods_until_method_support_is_selected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("tsvector_index_access_methods").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_tsvector_index_rows (document TEXT)")
            .await
            .expect("create a text table for the unsupported SQL access methods");

        // Act
        // Assert
        assert_unselected_index_is_rejected(
            &client,
            "CREATE INDEX compat_tsvector_gin_idx \
             ON compat_tsvector_index_rows USING gin (document)",
            "compat_tsvector_gin_idx",
        )
        .await;
        assert_unselected_index_is_rejected(
            &client,
            "CREATE INDEX compat_tsvector_gist_idx \
             ON compat_tsvector_index_rows USING gist (document)",
            "compat_tsvector_gist_idx",
        )
        .await;

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_gist_inet_operator_class_until_method_support_is_selected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("inet_gist_index").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_inet_gist_index_rows (address INET)")
            .await
            .expect("create an inet table for the unsupported GiST method");

        // Act
        // Assert
        assert_unselected_index_is_rejected(
            &client,
            "CREATE INDEX compat_inet_gist_idx \
             ON compat_inet_gist_index_rows USING gist (address inet_ops)",
            "compat_inet_gist_idx",
        )
        .await;

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_brin_operator_class_parameters_until_method_support_is_selected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("brin_opclass_parameter").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_brin_parameter_rows (id INT)")
            .await
            .expect("create an integer table for the unsupported BRIN method");

        // Act
        // Assert
        assert_unselected_index_is_rejected(
            &client,
            "CREATE INDEX compat_brin_parameter_idx \
             ON compat_brin_parameter_rows USING brin \
             (id int4_minmax_multi_ops (values_per_range = 8))",
            "compat_brin_parameter_idx",
        )
        .await;

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_spgist_inet_operator_class_until_method_support_is_selected() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("spgist_inet_index").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_spgist_inet_index_rows (address INET)")
            .await
            .expect("create an inet table for the unsupported SP-GiST method");

        // Act
        // Assert
        assert_unselected_index_is_rejected(
            &client,
            "CREATE INDEX compat_spgist_inet_idx \
             ON compat_spgist_inet_index_rows USING spgist (address)",
            "compat_spgist_inet_idx",
        )
        .await;

        drop(client);
        server.shutdown(connection).await;
    });
}
