use super::*;

#[test]
fn should_generate_smallserial_with_int2_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("smallserial_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &["CREATE TABLE compat_smallserial_rows (id SMALLSERIAL PRIMARY KEY)"],
        )
        .await
        .expect("create a smallserial column");

        // Act
        let smallserial = client
            .query_one(
                "INSERT INTO compat_smallserial_rows DEFAULT VALUES RETURNING id",
                &[],
            )
            .await
            .expect("allocate the first smallserial value");

        // Assert
        assert_eq!(smallserial.columns()[0].type_().oid(), 21);
        assert_eq!(smallserial.get::<_, i16>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_generate_serial2_with_int2_identity() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("serial2_identity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_serial2_rows (id SERIAL2 PRIMARY KEY)")
            .await
            .expect("create a serial2 column");

        // Act
        let serial2 = client
            .query_one(
                "INSERT INTO compat_serial2_rows DEFAULT VALUES RETURNING id",
                &[],
            )
            .await
            .expect("allocate the first serial2 value");

        // Assert
        assert_eq!(serial2.columns()[0].type_().oid(), 21);
        assert_eq!(serial2.get::<_, i16>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
