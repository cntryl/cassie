#[test]
fn should_change_sequence_persistence_using_alter_sequence() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_persistence_modes").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE UNLOGGED SEQUENCE compat_wave7_persistence_sequence")
            .await
            .expect("create an unlogged sequence");

        // Act
        let initially_unlogged: String = client
            .query_one(
                "SELECT relpersistence::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_wave7_persistence_sequence'::regclass",
                &[],
            )
            .await
            .expect("read initial sequence persistence")
            .try_get(0)
            .expect("initial persistence mode");
        client
            .batch_execute("ALTER SEQUENCE compat_wave7_persistence_sequence SET LOGGED")
            .await
            .expect("make the sequence logged");
        let logged: String = client
            .query_one(
                "SELECT relpersistence::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_wave7_persistence_sequence'::regclass",
                &[],
            )
            .await
            .expect("read logged sequence persistence")
            .try_get(0)
            .expect("logged persistence mode");
        client
            .batch_execute("ALTER SEQUENCE compat_wave7_persistence_sequence SET UNLOGGED")
            .await
            .expect("make the sequence unlogged again");
        let unlogged_again: String = client
            .query_one(
                "SELECT relpersistence::text FROM pg_catalog.pg_class \
                 WHERE oid = 'compat_wave7_persistence_sequence'::regclass",
                &[],
            )
            .await
            .expect("read final sequence persistence")
            .try_get(0)
            .expect("final persistence mode");
        let first = client
            .query_one("SELECT nextval('compat_wave7_persistence_sequence')", &[])
            .await
            .expect("allocate from the unlogged sequence");

        // Assert
        assert_eq!(initially_unlogged, "u");
        assert_eq!(logged, "p");
        assert_eq!(unlogged_again, "u");
        assert_eq!(first.get::<_, i64>(0), 1);
        assert_eq!(first.columns()[0].type_().oid(), 20);

        drop(client);
        server.shutdown(connection).await;
    });
}
