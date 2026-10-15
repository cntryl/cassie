#[test]
fn should_revoke_sequence_usage_without_advancing_the_sequence() {
    // Arrange
    let runtime = lifecycle_wave_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_usage_revocation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE ROLE compat_wave2_sequence_reader; \
                 CREATE SEQUENCE compat_wave2_sequence; \
                 GRANT USAGE ON SEQUENCE compat_wave2_sequence \
                   TO compat_wave2_sequence_reader",
            )
            .await
            .expect("create a sequence reader and grant USAGE");
        client
            .batch_execute("SET ROLE compat_wave2_sequence_reader")
            .await
            .expect("assume the sequence reader role");
        let first = client
            .query_one("SELECT nextval('compat_wave2_sequence')", &[])
            .await
            .expect("USAGE allows nextval");
        client
            .batch_execute(
                "RESET ROLE; REVOKE USAGE ON SEQUENCE compat_wave2_sequence \
                            FROM compat_wave2_sequence_reader; \
                            SET ROLE compat_wave2_sequence_reader",
            )
            .await
            .expect("revoke USAGE and re-assume the reader role");

        // Act
        let denied = client
            .query_one("SELECT nextval('compat_wave2_sequence')", &[])
            .await
            .expect_err("nextval requires sequence permission after USAGE is revoked");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role after the denied call");
        let sequence_state = client
            .query_one(
                "SELECT last_value, is_called FROM compat_wave2_sequence",
                &[],
            )
            .await
            .expect("read sequence state after the denied call");

        // Assert
        assert_eq!(first.get::<_, i64>(0), 1);
        assert_eq!(db_error(&denied).code().code(), "42501");
        assert_eq!(sequence_state.get::<_, i64>(0), 1);
        assert!(sequence_state.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}
