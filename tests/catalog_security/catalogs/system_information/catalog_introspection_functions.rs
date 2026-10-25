#[test]
fn should_report_current_catalog_as_the_active_database() {
    // Arrange
    let fixture = sql_fixture("issue-817-current-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT current_catalog, current_database(), \
                    current_catalog = current_database()",
        )
        .expect("read the current catalog name and compare it with the active database");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![19, 19, 16]
    );
    assert_eq!(
        selected.rows(),
        &[vec![
            Value::String("postgres".to_owned()),
            Value::String("postgres".to_owned()),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_report_the_negotiated_client_encoding_over_pgwire() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let (server, client, connection, path) =
            start_pgwire_fixture("issue-817-client-encoding").await;

        // Act
        let database = client
            .query_one("SELECT current_database()", &[])
            .await
            .expect("read the current database name over binary pgwire");
        assert_eq!(database.columns()[0].type_().oid(), 19);
        assert_eq!(database.get::<_, String>(0), "postgres");
        let row = client
            .query_one(
                "SELECT pg_catalog.pg_client_encoding(), \
                        current_setting('client_encoding')",
                &[],
            )
            .await
            .expect("read the negotiated client encoding and session setting");

        // Assert
        assert_eq!(
            row.columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![19, 25]
        );
        assert_eq!(
            (row.get::<_, String>(0), row.get::<_, String>(1),),
            ("UTF8".to_owned(), "UTF8".to_owned())
        );

        stop_pgwire_fixture(server, client, connection, path).await;
    });
}

#[test]
fn should_describe_pg_client_encoding_with_native_name_identity() {
    // Arrange
    let fixture = sql_fixture("issue-817-client-encoding-identity", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT pg_catalog.pg_client_encoding()")
        .expect("read the client encoding name");

    // Assert
    assert_eq!(selected.columns()[0].type_oid, 19);
    assert_eq!(selected.rows(), &[vec![Value::String("UTF8".to_owned())]]);
}
