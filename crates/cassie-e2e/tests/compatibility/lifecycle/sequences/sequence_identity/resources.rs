#[test]
fn should_preserve_smallint_sequence_type_limits() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("smallint_sequence_contract").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE SEQUENCE compat_wave7_smallint_sequence AS SMALLINT \
                 START WITH 32766 MINVALUE 32766 MAXVALUE 32767 NO CYCLE",
            )
            .await
            .expect("create a bounded smallint sequence");

        // Act
        let first = client
            .query_one("SELECT nextval('compat_wave7_smallint_sequence')", &[])
            .await
            .expect("read the first smallint sequence value");
        let second: i64 = client
            .query_one("SELECT nextval('compat_wave7_smallint_sequence')", &[])
            .await
            .expect("read the maximum smallint sequence value")
            .try_get(0)
            .expect("second sequence value");
        let exhausted = client
            .query_one("SELECT nextval('compat_wave7_smallint_sequence')", &[])
            .await
            .expect_err("a noncycling sequence must stop at its maximum");
        let metadata = client
            .query_one(
                "SELECT data_type, start_value::text, minimum_value::text, \
                        maximum_value::text, increment::text, cycle_option \
                 FROM information_schema.sequences \
                 WHERE sequence_schema = 'public' \
                   AND sequence_name = 'compat_wave7_smallint_sequence'",
                &[],
            )
            .await
            .expect("read sequence type and option metadata");

        // Assert
        // PostgreSQL 18.6 keeps nextval's result as bigint (OID 20) for a smallint sequence.
        assert_eq!(first.get::<_, i64>(0), 32766);
        assert_eq!(first.columns()[0].type_().oid(), 20);
        assert_eq!(second, 32767);
        assert_eq!(db_error(&exhausted).code().code(), "2200H");
        assert_eq!(metadata.get::<_, String>(0), "smallint");
        assert_eq!(metadata.get::<_, String>(1), "32766");
        assert_eq!(metadata.get::<_, String>(2), "32766");
        assert_eq!(metadata.get::<_, String>(3), "32767");
        assert_eq!(metadata.get::<_, String>(4), "1");
        assert_eq!(metadata.get::<_, String>(5), "NO");

        drop(client);
        server.shutdown(connection).await;
    });
}
