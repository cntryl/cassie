use super::*;

async fn create_unpartitioned_relation(
    client: &tokio_postgres::Client,
) -> Result<(), tokio_postgres::Error> {
    client
        .batch_execute(
            "CREATE TABLE compat_partition_information_regular \
             (id INT NOT NULL, bucket TEXT NOT NULL)",
        )
        .await
}

async fn create_nested_partition_information_fixture(
    client: &tokio_postgres::Client,
) -> Result<(), tokio_postgres::Error> {
    for statement in [
        "CREATE TABLE compat_partition_information_root (key INT NOT NULL, bucket TEXT NOT NULL) \
         PARTITION BY RANGE (key)",
        "CREATE TABLE compat_partition_information_early \
         PARTITION OF compat_partition_information_root \
         FOR VALUES FROM (MINVALUE) TO (100) PARTITION BY LIST (bucket)",
        "CREATE TABLE compat_partition_information_late \
         PARTITION OF compat_partition_information_root \
         FOR VALUES FROM (100) TO (MAXVALUE)",
        "CREATE TABLE compat_partition_information_early_a \
         PARTITION OF compat_partition_information_early FOR VALUES IN ('a')",
        "CREATE TABLE compat_partition_information_early_b \
         PARTITION OF compat_partition_information_early FOR VALUES IN ('b')",
        "INSERT INTO compat_partition_information_root VALUES \
         (1, 'a'), (2, 'b'), (101, 'z')",
    ] {
        client.batch_execute(statement).await?;
    }

    Ok(())
}

#[test]
fn should_return_an_empty_partition_tree_for_an_unpartitioned_relation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partition_tree_information").await;
        let (client, connection) = server.connect().await;
        let fixture = create_unpartitioned_relation(&client).await;

        // Act
        let tree_statement = if fixture.is_ok() {
            Some(
                client
                    .prepare(
                        "SELECT pg_catalog.pg_partition_tree(\
                           'compat_partition_information_regular') AS partition_tree",
                    )
                    .await,
            )
        } else {
            None
        };
        let tree_rows = match &tree_statement {
            Some(Ok(statement)) => Some(client.query(statement, &[]).await),
            Some(Err(_)) | None => None,
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            fixture.is_ok(),
            "create the ordinary relation for partition-function coverage: {fixture:?}"
        );
        let tree_statement = tree_statement
            .expect("prepare pg_partition_tree after creating its relation")
            .expect("prepare the pg_partition_tree projection");
        assert_eq!(tree_statement.columns()[0].name(), "partition_tree");
        assert_eq!(tree_statement.columns()[0].type_().oid(), 2249);
        assert!(
            tree_rows
                .expect("execute pg_partition_tree after preparing it")
                .expect("read the tree for an unpartitioned relation")
                .is_empty(),
            "PostgreSQL 18.6 returns no pg_partition_tree rows for an unpartitioned relation"
        );
    });
}

#[test]
fn should_return_null_root_for_an_unpartitioned_relation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partition_root_information").await;
        let (client, connection) = server.connect().await;
        let fixture = create_unpartitioned_relation(&client).await;

        // Act
        let root_value = if fixture.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT pg_catalog.pg_partition_root(\
                           'compat_partition_information_regular')::text AS root_relation",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let descriptor = if fixture.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT pg_catalog.pg_partition_root(\
                           'compat_partition_information_regular') AS root_relation",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            fixture.is_ok(),
            "create the ordinary relation for partition-function coverage: {fixture:?}"
        );
        let root_value = root_value
            .expect("query the root after creating its relation")
            .expect("read the root for an unpartitioned relation");
        assert_eq!(root_value.get::<_, Option<String>>(0), None);
        let descriptor = descriptor
            .expect("request pg_partition_root output metadata")
            .expect("describe the partition root result");
        assert_eq!(descriptor.columns()[0].name(), "root_relation");
        assert_eq!(descriptor.columns()[0].type_().oid(), 2205);
    });
}

#[test]
fn should_return_no_partition_ancestors_for_an_unpartitioned_relation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partition_ancestors_information").await;
        let (client, connection) = server.connect().await;
        let fixture = create_unpartitioned_relation(&client).await;

        // Act
        let ancestors_statement = if fixture.is_ok() {
            Some(
                client
                    .prepare(
                        "SELECT pg_catalog.pg_partition_ancestors(\
                           'compat_partition_information_regular') AS relid",
                    )
                    .await,
            )
        } else {
            None
        };
        let ancestor_rows = match &ancestors_statement {
            Some(Ok(statement)) => Some(client.query(statement, &[]).await),
            Some(Err(_)) | None => None,
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            fixture.is_ok(),
            "create the ordinary relation for partition-function coverage: {fixture:?}"
        );
        let ancestors_statement = ancestors_statement
            .expect("prepare pg_partition_ancestors after creating its relation")
            .expect("prepare the pg_partition_ancestors projection");
        assert_eq!(ancestors_statement.columns()[0].name(), "relid");
        assert_eq!(ancestors_statement.columns()[0].type_().oid(), 2205);
        assert!(
            ancestor_rows
                .expect("execute pg_partition_ancestors after preparing it")
                .expect("read ancestors for an unpartitioned relation")
                .is_empty(),
            "PostgreSQL 18.6 returns no pg_partition_ancestors rows for an unpartitioned relation"
        );
    });
}

#[test]
fn should_create_a_nested_partition_hierarchy_as_ddl_acceptance_probe() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("nested_partition_ddl_probe").await;
        let (client, connection) = server.connect().await;

        // This is a separate partition-DDL probe. It does not call or qualify any
        // pg_partition_* function; a parser error here is a DDL blocker only.
        // Act
        let fixture = create_nested_partition_information_fixture(&client).await;

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            fixture.is_ok(),
            "create the nested partition hierarchy: {fixture:?}"
        );
    });
}
