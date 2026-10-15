use super::*;
use cassie::types::Value;

#[test]
fn should_apply_encoding_conversion_given_a_sql_conversion_object_when_text_is_converted() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("named_encoding_conversion").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE CONVERSION compat_utf8_to_latin1 \
                 FOR 'UTF8' TO 'LATIN1' FROM pg_catalog.utf8_to_iso8859_1",
            )
            .await
            .expect("create the UTF8 to LATIN1 conversion object");

        // Act
        let converted = client
            .query_one(
                "SELECT encode(convert(convert_to('cafè', 'UTF8'), \
                 'UTF8', 'LATIN1'), 'hex')",
                &[],
            )
            .await
            .expect("convert UTF8 bytes with the named conversion object")
            .get::<_, String>(0);

        // Assert
        assert_eq!(converted, "636166e8");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_conversion_resolution_given_conversion_ddl_when_the_transaction_rolls_back() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conversion_ddl_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a conversion catalog transaction");
        client
            .batch_execute(
                "CREATE CONVERSION compat_rolled_back_conversion \
                 FOR 'UTF8' TO 'LATIN1' FROM pg_catalog.utf8_to_iso8859_1",
            )
            .await
            .expect("create a conversion inside the transaction");
        let visible_in_transaction: i64 = client
            .query_one(
                "SELECT count(*)::BIGINT FROM pg_catalog.pg_conversion \
                 WHERE conname = 'compat_rolled_back_conversion'",
                &[],
            )
            .await
            .expect("read the conversion catalog row before rollback")
            .get(0);

        // Act
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back conversion object creation");
        let after_rollback: i64 = client
            .query_one(
                "SELECT count(*)::BIGINT FROM pg_catalog.pg_conversion \
                 WHERE conname = 'compat_rolled_back_conversion'",
                &[],
            )
            .await
            .expect("read conversion catalog after rollback")
            .get(0);

        // Assert
        assert_eq!(visible_in_transaction, 1);
        assert_eq!(after_rollback, 0);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_conversion_dependencies_given_persisted_conversion_objects_when_storage_restarts(
) {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("encoding_conversion_restart");
    let _directory = DataDirectory(path.clone());
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
    cassie.startup().expect("start Cassie instance");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE CONVERSION compat_utf8_to_latin1 \
             FOR 'UTF8' TO 'LATIN1' FROM pg_catalog.utf8_to_iso8859_1",
            vec![],
        )
        .expect("create the UTF8 to LATIN1 conversion object");
    drop(session);
    drop(cassie);

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    restarted
        .startup()
        .expect("recover conversion catalog objects");
    let session = restarted.create_session("tester", None);
    let result = restarted
        .execute_sql(
            &session,
            "SELECT encode(convert(convert_to('cafè', 'UTF8'), 'UTF8', 'LATIN1'), 'hex')",
            vec![],
        )
        .expect("resolve and use the persisted named conversion")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(result, vec![vec![Value::String("636166e8".to_owned())]]);
}

#[test]
fn should_remove_conversion_metadata_given_a_conversion_object_when_it_is_dropped() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conversion_drop_metadata").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE CONVERSION compat_dropped_conversion \
                 FOR 'UTF8' TO 'LATIN1' FROM pg_catalog.utf8_to_iso8859_1",
            )
            .await
            .expect("create the conversion object");

        // Act
        client
            .batch_execute("DROP CONVERSION compat_dropped_conversion")
            .await
            .expect("drop the named conversion object");
        let remaining: i64 = client
            .query_one(
                "SELECT count(*)::BIGINT FROM pg_catalog.pg_conversion \
                 WHERE conname = 'compat_dropped_conversion'",
                &[],
            )
            .await
            .expect("read the conversion catalog after drop")
            .get(0);

        // Assert
        assert_eq!(remaining, 0);
        drop(client);
        server.shutdown(connection).await;
    });
}
