#[test]
fn should_describe_namespace_identity_in_text_and_binary_results() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let (server, client, connection, path) = start_pgwire_fixture("namespace_descriptor_wire").await;
        client.batch_execute("CREATE SCHEMA descriptor_schema").await.expect("create namespace");

        // Act
        let statement = client.prepare("SELECT oid, nspname FROM pg_catalog.pg_namespace WHERE nspname = 'descriptor_schema'").await.expect("prepare descriptor");
        let binary = client.query_one(&statement, &[]).await.expect("binary row");
        let empty = client.prepare("SELECT oid, nspname FROM pg_catalog.pg_namespace WHERE nspname = 'missing_schema'").await.expect("prepare empty descriptor");
        let empty_rows = client.query(&empty, &[]).await.expect("empty rows");
        let text = client.simple_query("SELECT oid, nspname FROM pg_catalog.pg_namespace WHERE nspname = 'descriptor_schema'").await.expect("text row");

        // Assert
        assert_eq!(statement.columns().iter().map(|column| column.type_().oid()).collect::<Vec<_>>(), vec![26, 19]);
        assert_eq!(empty.columns().iter().map(|column| column.type_().oid()).collect::<Vec<_>>(), vec![26, 19]);
        assert!(empty_rows.is_empty());
        let oid = binary.get::<_, u32>(0);
        assert_ne!(oid, 0);
        assert_eq!(binary.get::<_, String>(1), "descriptor_schema");
        let text_row = text.iter().find_map(|message| match message { tokio_postgres::SimpleQueryMessage::Row(row) => Some(row), _ => None }).expect("text result row");
        let oid_text = oid.to_string();
        assert_eq!(text_row.get(0), Some(oid_text.as_str()));
        assert_eq!(text_row.get(1), Some("descriptor_schema"));
        stop_pgwire_fixture(server, client, connection, path).await;
    });
}

#[test]
fn should_keep_namespace_descriptor_and_rows_database_scoped() {
    // Arrange
    let fixture = sql_fixture(
        "namespace_descriptor_database_scope",
        &["CREATE DATABASE namespace_tenant"],
    );
    let tenant = fixture
        .cassie
        .create_session("tester", Some("namespace_tenant".to_owned()));
    fixture
        .cassie
        .execute_sql(&tenant, "CREATE SCHEMA tenant_only", vec![])
        .expect("create tenant namespace");

    // Act
    let owner = fixture
        .cassie
        .execute_sql(
            &tenant,
            "SELECT oid, nspname FROM pg_catalog.pg_namespace WHERE nspname = 'tenant_only'",
            vec![],
        )
        .expect("tenant rows");
    let other = fixture
        .execute("SELECT oid, nspname FROM pg_catalog.pg_namespace WHERE nspname = 'tenant_only'")
        .expect("postgres rows");

    // Assert
    assert_eq!(
        owner
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![26, 19]
    );
    assert_eq!(owner.rows().len(), 1);
    assert!(other.rows().is_empty());
}

#[test]
fn should_preserve_namespace_oid_above_signed_integer_range() {
    // Arrange
    let fixture = sql_fixture("namespace_descriptor_unsigned_oid", &[]);
    fixture
        .cassie
        .catalog
        .register_namespace_meta(cassie::catalog::NamespaceMeta {
            name: "postgres.high_oid".to_owned(),
            description: None,
            object_id: u64::from(u32::MAX) - 16_384,
        });

    // Act
    let result = fixture
        .execute("SELECT oid FROM pg_catalog.pg_namespace WHERE nspname = 'high_oid'")
        .expect("namespace OID");

    // Assert
    assert_eq!(result.columns()[0].type_oid, 26);
    assert_eq!(result.rows(), &[vec![Value::Int64(i64::from(u32::MAX))]]);
}

#[test]
fn should_round_trip_maximum_namespace_oid_in_binary_pgwire() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = support_sql::new_in_memory_cassie().expect("Cassie");
        cassie.startup().expect("startup");
        cassie
            .catalog
            .register_namespace_meta(cassie::catalog::NamespaceMeta {
                name: "postgres.wire_high_oid".to_owned(),
                description: None,
                object_id: u64::from(u32::MAX) - 16_384,
            });
        let server = support_pgwire::spawn_server(cassie).await;
        let mut config = tokio_postgres::Config::new();
        config
            .host("127.0.0.1")
            .port(server.addr.port())
            .user("root")
            .password(crate::support_sql::test_root_password())
            .dbname("postgres");
        let (client, connection) = config.connect(NoTls).await.expect("connect");
        let connection = tokio::spawn(connection);

        // Act
        let row = client
            .query_one(
                "SELECT oid FROM pg_catalog.pg_namespace WHERE nspname = 'wire_high_oid'",
                &[],
            )
            .await
            .expect("binary OID");

        // Assert
        assert_eq!(row.columns()[0].type_().oid(), 26);
        assert_eq!(row.get::<_, u32>(0), u32::MAX);
        drop(client);
        connection.abort();
        let _ = connection.await;
        server.stop().await;
    });
}

#[test]
fn should_join_and_preserve_namespace_identity_across_rollback() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let (server, client, connection, path) = start_pgwire_fixture("namespace_descriptor_join").await;
        client.batch_execute("CREATE SCHEMA descriptor_before; CREATE TABLE descriptor_before.rows (v INT)").await.expect("create schema and table");
        let before = client.query_one("SELECT n.oid FROM pg_catalog.pg_namespace n JOIN pg_catalog.pg_class c ON n.oid = c.relnamespace_oid WHERE n.nspname = 'descriptor_before' AND c.relname = 'rows'", &[]).await.expect("join catalog identities").get::<_, u32>(0);

        // Act
        client.batch_execute("BEGIN; ALTER SCHEMA descriptor_before RENAME TO descriptor_after; ROLLBACK").await.expect("rollback rename");
        let after = client.query_one("SELECT oid FROM pg_catalog.pg_namespace WHERE nspname = 'descriptor_before'", &[]).await.expect("restored namespace").get::<_, u32>(0);

        // Assert
        assert_ne!(before, 0);
        assert_eq!(after, before);
        stop_pgwire_fixture(server, client, connection, path).await;
    });
}
