use super::*;

fn large_object_wire_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread Tokio runtime")
}

#[test]
fn should_preserve_postgresql_wire_oids_for_large_object_io() {
    // Arrange
    let runtime = large_object_wire_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_wire_oids").await;
        let (client, connection) = server.connect().await;
        let created = client
            .query_one("SELECT lo_from_bytea(0, decode('6162', 'hex'))", &[])
            .await
            .expect("create a large object with a known byte sequence");
        let object_id: u32 = created.try_get(0).expect("large-object OID value");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin large-object descriptor transaction");
        let descriptor = client
            .query_one(&format!("SELECT lo_open({object_id}::oid, 393216)"), &[])
            .await
            .expect("open the large object for reading and writing");
        let descriptor_id: i32 = descriptor.try_get(0).expect("descriptor value");

        // Act
        let read = client
            .query_one(&format!("SELECT loread({descriptor_id}, 1)"), &[])
            .await
            .expect("read one byte from the descriptor");
        let read_bytes: Vec<u8> = read.try_get(0).expect("large-object read bytes");
        let written = client
            .query_one(
                &format!("SELECT lowrite({descriptor_id}, decode('ff', 'hex'))"),
                &[],
            )
            .await
            .expect("write one byte at the current descriptor position");
        let seek = client
            .query_one(
                &format!("SELECT lo_lseek64({descriptor_id}, 0::bigint, 0)"),
                &[],
            )
            .await
            .expect("seek to the beginning with the 64-bit function");
        let position: i64 = client
            .query_one(&format!("SELECT lo_tell64({descriptor_id})"), &[])
            .await
            .expect("read the 64-bit descriptor position")
            .try_get(0)
            .expect("64-bit position");
        let truncated = client
            .query_one(
                &format!("SELECT lo_truncate64({descriptor_id}, 3::bigint)"),
                &[],
            )
            .await
            .expect("extend the object with the 64-bit truncate function");
        let closed = client
            .query_one(&format!("SELECT lo_close({descriptor_id})"), &[])
            .await
            .expect("close the large-object descriptor");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit large-object descriptor transaction");
        let bytes: Vec<u8> = client
            .query_one(&format!("SELECT lo_get({object_id}::oid)"), &[])
            .await
            .expect("read the final object bytes")
            .try_get(0)
            .expect("final object bytea");

        // Assert
        // PostgreSQL 18.6 wire OIDs: oid=26, int4=23, bytea=17, int8=20.
        assert_eq!(created.columns()[0].type_().oid(), 26);
        assert_eq!(descriptor.columns()[0].type_().oid(), 23);
        assert_eq!(read.columns()[0].type_().oid(), 17);
        assert_eq!(written.columns()[0].type_().oid(), 23);
        assert_eq!(seek.columns()[0].type_().oid(), 20);
        assert_eq!(seek.try_get::<_, i64>(0).expect("seek result"), 0);
        assert_eq!(position, 0);
        assert_eq!(truncated.columns()[0].type_().oid(), 23);
        assert_eq!(truncated.try_get::<_, i32>(0).expect("truncate result"), 0);
        assert_eq!(closed.columns()[0].type_().oid(), 23);
        assert_eq!(closed.try_get::<_, i32>(0).expect("close result"), 0);
        assert_eq!(read_bytes, [0x61]);
        assert_eq!(written.try_get::<_, i32>(0).expect("write result"), 1);
        assert_eq!(bytes, [0x61, 0xff, 0x00]);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_missing_large_object_open_with_postgresql_sqlstate() {
    // Arrange
    let runtime = large_object_wire_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_missing_open_sqlstate").await;
        let (client, connection) = server.connect().await;

        // Act
        let missing = client
            .query_one("SELECT lo_open(4294967295::oid, 262144)", &[])
            .await
            .expect_err("opening a missing large object must fail");

        // Assert
        // PostgreSQL 18.6 returns 42704 (undefined_object) for a missing LO OID.
        assert_eq!(db_error(&missing).code().code(), "42704");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_invalidate_large_object_descriptor_at_transaction_commit() {
    // Arrange
    let runtime = large_object_wire_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_descriptor_commit_lifetime").await;
        let (client, connection) = server.connect().await;
        let created = client
            .query_one("SELECT lo_from_bytea(0, decode('6162', 'hex'))", &[])
            .await
            .expect("create a large object with known bytes");
        let object_id: u32 = created.try_get(0).expect("large-object OID");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin descriptor-lifetime transaction");
        let opened = client
            .query_one(&format!("SELECT lo_open({object_id}::oid, 262144)"), &[])
            .await
            .expect("open the object for reading inside the transaction");
        let descriptor_id: i32 = opened.try_get(0).expect("large-object descriptor");
        let in_transaction_read = client
            .query_one(&format!("SELECT loread({descriptor_id}, 1)"), &[])
            .await
            .expect("read through the descriptor before its transaction ends");
        let in_transaction_bytes: Vec<u8> = in_transaction_read
            .try_get(0)
            .expect("in-transaction large-object bytes");
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit the descriptor-lifetime transaction");

        // Act
        let expired_read = client
            .query_one(&format!("SELECT loread({descriptor_id}, 1)"), &[])
            .await
            .expect_err("transaction completion must close the descriptor");
        let persisted = client
            .query_one(&format!("SELECT lo_get({object_id}::oid)"), &[])
            .await
            .expect("read the object after the descriptor expires");
        let persisted_bytes: Vec<u8> = persisted
            .try_get(0)
            .expect("large-object bytes survive descriptor closure");
        let unlinked = client
            .query_one(&format!("SELECT lo_unlink({object_id}::oid)"), &[])
            .await
            .expect("remove the fixture large object");

        // Assert
        // PostgreSQL 18.6 reports 42704 for use of a descriptor after commit.
        assert_eq!(in_transaction_bytes, [0x61]);
        assert_eq!(db_error(&expired_read).code().code(), "42704");
        assert_eq!(persisted_bytes, [0x61, 0x62]);
        assert_eq!(unlinked.try_get::<_, i32>(0).expect("unlink status"), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
