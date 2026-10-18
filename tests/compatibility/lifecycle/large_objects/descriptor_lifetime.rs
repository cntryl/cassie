use super::*;

#[test]
fn should_invalidate_descriptors_given_an_aborted_transaction_when_large_object_io_is_retried() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_abort_descriptor").await;
        let (client, connection) = server.connect().await;
        let oid: i32 = client
            .query_one("SELECT lo_create(0)::INT", &[])
            .await
            .expect("create a large object outside the transaction")
            .get(0);
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for the descriptor lifetime");
        let descriptor: i32 = client
            .query_one("SELECT lo_open($1::INT4::OID, 131072)", &[&oid])
            .await
            .expect("open the object in the transaction")
            .get(0);

        // Act
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("end the transaction and its descriptor");
        let read = client
            .query_one("SELECT loread($1, 1)", &[&descriptor])
            .await;

        // Assert
        assert_eq!(
            db_error(
                read.as_ref()
                    .expect_err("the descriptor is closed by rollback")
            )
            .code()
            .code(),
            "42704"
        );
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_isolate_descriptor_lifetime_given_a_disconnected_session_when_another_session_attempts_io(
) {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_session_descriptor").await;
        let (owner, owner_connection) = server.connect().await;
        let oid: i32 = owner
            .query_one("SELECT lo_create(0)::INT", &[])
            .await
            .expect("create a large object before opening a session descriptor")
            .get(0);
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for the session descriptor");
        let descriptor: i32 = owner
            .query_one("SELECT lo_open($1::INT4::OID, 131072)", &[&oid])
            .await
            .expect("open the object in the first session")
            .get(0);
        drop(owner);
        owner_connection
            .await
            .expect("the first session should close after its client is dropped");
        let (observer, observer_connection) = server.connect().await;

        // Act
        let read = observer
            .query_one("SELECT loread($1, 1)", &[&descriptor])
            .await;

        // Assert
        assert_eq!(
            db_error(
                read.as_ref()
                    .expect_err("large-object descriptors are scoped to their opening session")
            )
            .code()
            .code(),
            "42704"
        );
        drop(observer);
        server.shutdown(observer_connection).await;
    });
}
