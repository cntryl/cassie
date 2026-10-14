use super::*;

mod concurrent_detach_finalize;

#[test]
fn should_publish_multicolumn_partition_identity_in_catalogs() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue840_partition_catalog_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat840_catalog_root (id INT, region TEXT) \
                   PARTITION BY RANGE (id, region)",
                "CREATE TABLE compat840_catalog_child PARTITION OF compat840_catalog_root \
                   FOR VALUES FROM (MINVALUE, MINVALUE) TO (10, MINVALUE)",
            ],
        )
        .await
        .expect("create a two-column range partition hierarchy");

        // Act
        let statement = client
            .prepare(
                "SELECT parent.relname::text, child.relname::text, child.relispartition, \
                        inheritance.inhparent = parent.oid, \
                        inheritance.inhrelid = child.oid, \
                        partitioning.partstrat::text, partitioning.partnatts::int, \
                        partitioning.partattrs::text \
                 FROM pg_catalog.pg_class AS parent \
                 JOIN pg_catalog.pg_partitioned_table AS partitioning \
                   ON partitioning.partrelid = parent.oid \
                 JOIN pg_catalog.pg_inherits AS inheritance \
                   ON inheritance.inhparent = parent.oid \
                 JOIN pg_catalog.pg_class AS child \
                   ON child.oid = inheritance.inhrelid \
                 WHERE parent.oid = 'compat840_catalog_root'::regclass \
                   AND child.oid = 'compat840_catalog_child'::regclass",
            )
            .await
            .expect("describe partition relationship and strategy metadata");
        let catalog_row = client
            .query_one(&statement, &[])
            .await
            .expect("read partition relationship and strategy metadata");

        // Assert
        assert_eq!(
            statement
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            [25, 25, 16, 16, 16, 25, 23, 25],
            "catalog expressions expose text, bool, and int4 result identities"
        );
        assert_eq!(catalog_row.get::<_, String>(0), "compat840_catalog_root");
        assert_eq!(catalog_row.get::<_, String>(1), "compat840_catalog_child");
        assert!(catalog_row.get::<_, bool>(2));
        assert!(catalog_row.get::<_, bool>(3));
        assert!(catalog_row.get::<_, bool>(4));
        assert_eq!(catalog_row.get::<_, String>(5), "r");
        assert_eq!(catalog_row.get::<_, i32>(6), 2);
        assert_eq!(catalog_row.get::<_, String>(7), "1 2");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_attach_a_child_index_to_its_partitioned_index() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue840_partitioned_index_attach").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat840_index_root (id INT) PARTITION BY RANGE (id)",
                "CREATE TABLE compat840_index_low PARTITION OF compat840_index_root \
                   FOR VALUES FROM (MINVALUE) TO (10)",
                "CREATE INDEX compat840_index_parent ON ONLY compat840_index_root (id)",
                "CREATE INDEX compat840_index_child ON compat840_index_low (id)",
            ],
        )
        .await
        .expect("create the parent and child indexes before attaching them");

        // Act
        client
            .batch_execute(
                "ALTER INDEX compat840_index_parent \
                 ATTACH PARTITION compat840_index_child",
            )
            .await
            .expect("attach the child index to the partitioned index");
        let statement = client
            .prepare(
                "SELECT parent.relname::text, child.relname::text, index_metadata.indisvalid, \
                        inheritance.inhparent = parent.oid, \
                        inheritance.inhrelid = child.oid \
                 FROM pg_catalog.pg_class AS parent \
                 JOIN pg_catalog.pg_index AS index_metadata \
                   ON index_metadata.indexrelid = parent.oid \
                 JOIN pg_catalog.pg_inherits AS inheritance \
                   ON inheritance.inhparent = parent.oid \
                 JOIN pg_catalog.pg_class AS child \
                   ON child.oid = inheritance.inhrelid \
                 WHERE parent.relname = 'compat840_index_parent' \
                   AND child.relname = 'compat840_index_child'",
            )
            .await
            .expect("describe the attached index relationship");
        let index_relationship = client
            .query_one(&statement, &[])
            .await
            .expect("read the attached child index relationship");

        // Assert
        assert_eq!(
            statement
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            [25, 25, 16, 16, 16]
        );
        assert_eq!(
            index_relationship.get::<_, String>(0),
            "compat840_index_parent"
        );
        assert_eq!(
            index_relationship.get::<_, String>(1),
            "compat840_index_child"
        );
        assert!(index_relationship.get::<_, bool>(2));
        assert!(index_relationship.get::<_, bool>(3));
        assert!(index_relationship.get::<_, bool>(4));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_detach_a_range_partition_concurrently() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue840_concurrent_partition_detach").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat840_detach_root (id INT) PARTITION BY RANGE (id)",
                "CREATE TABLE compat840_detach_child PARTITION OF compat840_detach_root \
                   FOR VALUES FROM (MINVALUE) TO (MAXVALUE)",
                "INSERT INTO compat840_detach_root VALUES (1), (2)",
            ],
        )
        .await
        .expect("create and populate a single range partition");

        // Act
        let detached = client
            .batch_execute(
                "ALTER TABLE compat840_detach_root DETACH PARTITION \
                 compat840_detach_child CONCURRENTLY",
            )
            .await;
        let parent_count = if detached.is_ok() {
            Some(
                client
                    .query_one("SELECT count(*) FROM compat840_detach_root", &[])
                    .await,
            )
        } else {
            None
        };
        let child_count = if detached.is_ok() {
            Some(
                client
                    .query_one("SELECT count(*) FROM compat840_detach_child", &[])
                    .await,
            )
        } else {
            None
        };
        let inheritance_count = if detached.is_ok() {
            Some(
                client
                    .query_one(
                        "SELECT count(*) FROM pg_catalog.pg_inherits \
                         WHERE inhparent = 'compat840_detach_root'::regclass \
                           AND inhrelid = 'compat840_detach_child'::regclass",
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
            detached.is_ok(),
            "PostgreSQL 18.6 accepts DETACH PARTITION CONCURRENTLY: {detached:?}"
        );
        assert_eq!(
            parent_count
                .expect("count parent rows after concurrent detach")
                .expect("query the parent after concurrent detach")
                .get::<_, i64>(0),
            0
        );
        assert_eq!(
            child_count
                .expect("count detached child rows")
                .expect("query the detached relation")
                .get::<_, i64>(0),
            2
        );
        assert_eq!(
            inheritance_count
                .expect("inspect pg_inherits after concurrent detach")
                .expect("read the parent-child relationship")
                .get::<_, i64>(0),
            0
        );
    });
}
