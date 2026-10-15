use super::*;

#[test]
fn should_reject_writes_given_a_read_only_large_object_descriptor_when_lowrite_is_called() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_read_only_write").await;
        let (client, connection) = server.connect().await;
        let oid: i32 = client
            .query_one("SELECT lo_create(0)::INT", &[])
            .await
            .expect("create an empty large object")
            .get(0);
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the large-object descriptor transaction");
        let descriptor: i32 = client
            .query_one("SELECT lo_open($1::INT4::OID, 262144)", &[&oid])
            .await
            .expect("open the large object for reading")
            .get(0);

        // Act
        let write = client
            .query_one("SELECT lowrite($1, decode('4142', 'hex'))", &[&descriptor])
            .await;

        // Assert
        assert_eq!(
            db_error(
                write
                    .as_ref()
                    .expect_err("a read-only descriptor cannot write bytes")
            )
            .code()
            .code(),
            "55000"
        );
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_read_existing_bytes_given_a_write_only_large_object_descriptor_when_loread_is_called() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_write_only_read").await;
        let (client, connection) = server.connect().await;
        let oid: i32 = client
            .query_one("SELECT lo_create(0)::INT", &[])
            .await
            .expect("create an empty large object")
            .get(0);
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the large-object descriptor transaction");
        let descriptor: i32 = client
            .query_one("SELECT lo_open($1::INT4::OID, 131072)", &[&oid])
            .await
            .expect("open the large object for writing")
            .get(0);
        client
            .query_one("SELECT lowrite($1, decode('4142', 'hex'))", &[&descriptor])
            .await
            .expect("write a known payload through the write-only descriptor");
        client
            .query_one("SELECT lo_lseek64($1, 0, 0)", &[&descriptor])
            .await
            .expect("seek to the beginning of the object");

        // Act
        let payload: Vec<u8> = client
            .query_one("SELECT loread($1, 2)", &[&descriptor])
            .await
            .expect("PostgreSQL permits reading through this descriptor mode")
            .get(0);

        // Assert
        assert_eq!(payload, vec![0x41, 0x42]);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_return_empty_bytes_given_a_descriptor_at_eof_when_loread_is_called() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_read_eof").await;
        let (client, connection) = server.connect().await;
        let oid: i32 = client
            .query_one("SELECT lo_create(0)::INT", &[])
            .await
            .expect("create an empty large object")
            .get(0);
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the large-object descriptor transaction");
        let descriptor: i32 = client
            .query_one("SELECT lo_open($1::INT4::OID, 262144)", &[&oid])
            .await
            .expect("open the large object for reading")
            .get(0);

        // Act
        let byte_count: i32 = client
            .query_one("SELECT octet_length(loread($1, 4))", &[&descriptor])
            .await
            .expect("read from the empty object at EOF")
            .get(0);

        // Assert
        assert_eq!(byte_count, 0);
        drop(client);
        server.shutdown(connection).await;
    });
}
