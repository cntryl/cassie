#[test]
fn should_rollback_transactional_partition_attachment_detachment() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partition_attach_detach").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_attach_parent (id INT, label TEXT) PARTITION BY RANGE (id)",
                "CREATE TABLE compat_attach_part (id INT, label TEXT)",
                "INSERT INTO compat_attach_part VALUES (1, 'one')",
                "ALTER TABLE compat_attach_parent ATTACH PARTITION compat_attach_part \
                   FOR VALUES FROM (0) TO (10)",
            ],
        )
        .await
        .expect("attach a populated table as a partition");
        let attached: i64 = client
            .query_one("SELECT count(*) FROM compat_attach_parent", &[])
            .await
            .expect("read the attached row through the parent")
            .try_get(0)
            .expect("parent row count");

        // Act
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin transactional detach");
        client
            .batch_execute("ALTER TABLE compat_attach_parent DETACH PARTITION compat_attach_part")
            .await
            .expect("detach the partition inside a transaction");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("restore the attachment after rollback");
        let after_rollback: i64 = client
            .query_one("SELECT count(*) FROM compat_attach_parent", &[])
            .await
            .expect("read parent after rolled-back detach")
            .try_get(0)
            .expect("parent row count after rollback");
        client
            .batch_execute("ALTER TABLE compat_attach_parent DETACH PARTITION compat_attach_part")
            .await
            .expect("detach the partition permanently");
        let detached_parent: i64 = client
            .query_one("SELECT count(*) FROM compat_attach_parent", &[])
            .await
            .expect("read parent after detach")
            .try_get(0)
            .expect("detached parent row count");
        let detached_child: i64 = client
            .query_one("SELECT count(*) FROM compat_attach_part", &[])
            .await
            .expect("read rows retained by the detached table")
            .try_get(0)
            .expect("detached child row count");

        // Assert
        assert_eq!(attached, 1);
        assert_eq!(after_rollback, 1);
        assert_eq!(detached_parent, 0);
        assert_eq!(detached_child, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
