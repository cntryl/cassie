use super::*;

#[test]
fn should_enforce_large_object_acl_given_a_restricted_role_when_object_io_is_requested() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("large_object_acl").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE ROLE compat_large_object_reader")
            .await
            .expect("create a role for the large-object ACL check");
        let oid: i32 = client
            .query_one("SELECT lo_create(0)::INT", &[])
            .await
            .expect("create a large object owned by the session administrator")
            .get(0);
        client
            .batch_execute(&format!(
                "GRANT SELECT ON LARGE OBJECT {oid} TO compat_large_object_reader"
            ))
            .await
            .expect("grant read but not write access to the large object");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the transaction that owns open descriptors");
        client
            .batch_execute("SET ROLE compat_large_object_reader")
            .await
            .expect("switch to the role with read-only large-object access");
        let read_descriptor: i32 = client
            .query_one("SELECT lo_open($1::INT4::OID, 262144)", &[&oid])
            .await
            .expect("open the object with the granted read privilege")
            .get(0);
        client
            .query_one("SELECT lo_close($1)", &[&read_descriptor])
            .await
            .expect("close the read-only descriptor before checking writes");

        // Act
        let write = client
            .query_one("SELECT lo_open($1::INT4::OID, 131072)", &[&oid])
            .await;
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("end the transaction after the denied open");

        // Assert
        assert_eq!(
            db_error(
                write
                    .as_ref()
                    .expect_err("read-only ACL must deny write open")
            )
            .code()
            .code(),
            "42501"
        );
        drop(client);
        server.shutdown(connection).await;
    });
}
