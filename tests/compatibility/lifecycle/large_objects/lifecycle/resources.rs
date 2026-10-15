#[test]
fn should_zero_fill_large_object_after_truncation_growth() {
    // Arrange
    let runtime = lifecycle_wave_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_truncate_growth").await;
        let (client, connection) = server.connect().await;
        let object_id: i32 = client
            .query_one(
                "SELECT lo_from_bytea(0, decode('6162636465', 'hex'))::int4",
                &[],
            )
            .await
            .expect("create an object containing five known bytes")
            .try_get(0)
            .expect("large-object OID as int4");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin large-object truncate transaction");
        let descriptor: i32 = client
            .query_one(&format!("SELECT lo_open({object_id}, 131072)"), &[])
            .await
            .expect("open the object for writing")
            .try_get(0)
            .expect("large-object descriptor");

        // Act
        let truncated: i32 = client
            .query_one(&format!("SELECT lo_truncate({descriptor}, 3)"), &[])
            .await
            .expect("truncate the object to three bytes")
            .try_get(0)
            .expect("truncate status");
        let shortened: String = client
            .query_one(
                &format!("SELECT encode(lo_get({object_id}::oid), 'hex')"),
                &[],
            )
            .await
            .expect("read the truncated object")
            .try_get(0)
            .expect("shortened object bytes");
        let extended: i32 = client
            .query_one(&format!("SELECT lo_truncate({descriptor}, 7)"), &[])
            .await
            .expect("extend the object to seven bytes")
            .try_get(0)
            .expect("extension status");
        let grown: String = client
            .query_one(
                &format!("SELECT encode(lo_get({object_id}::oid), 'hex')"),
                &[],
            )
            .await
            .expect("read the grown object")
            .try_get(0)
            .expect("grown object bytes");
        let closed: i32 = client
            .query_one(&format!("SELECT lo_close({descriptor})"), &[])
            .await
            .expect("close the large-object descriptor")
            .try_get(0)
            .expect("close status");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit large-object truncate transaction");

        // Assert
        assert_eq!(truncated, 0);
        assert_eq!(shortened, "616263");
        assert_eq!(extended, 0);
        assert_eq!(grown, "61626300000000");
        assert_eq!(closed, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
