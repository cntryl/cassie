use super::*;

#[test]
fn should_reject_invalid_seek_bounds_given_an_open_large_object_when_a_seek_is_requested() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_seek_bounds").await;
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

        // Act
        let seek = client
            .query_one("SELECT lo_lseek64($1, -1, 0)", &[&descriptor])
            .await;

        // Assert
        assert_eq!(
            db_error(
                seek.as_ref()
                    .expect_err("negative absolute seek is invalid")
            )
            .code()
            .code(),
            "22023"
        );
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_invalid_truncate_bounds_given_an_open_large_object_when_truncation_is_requested() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_truncate_bounds").await;
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

        // Act
        let truncate = client
            .query_one("SELECT lo_truncate64($1, -1)", &[&descriptor])
            .await;

        // Assert
        assert_eq!(
            db_error(
                truncate
                    .as_ref()
                    .expect_err("negative large-object length is invalid")
            )
            .code()
            .code(),
            "22023"
        );
        drop(client);
        server.shutdown(connection).await;
    });
}
