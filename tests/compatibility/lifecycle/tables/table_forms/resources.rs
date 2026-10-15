#[test]
fn should_skip_missing_table_drop_in_fresh_memory_storage() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let data_dir = support::data_dir("memory_drop_absent_table_if_exists");
        let _data_directory = DataDirectory(data_dir.clone());
        let mut config = cassie::config::CassieRuntimeConfig::from_env()
            .expect("read runtime config for memory-backed fixture");
        config.password = "postgres".to_string();

        let previous_storage_mode = std::env::var_os("CASSIE_STORAGE_MODE");
        std::env::set_var("CASSIE_STORAGE_MODE", "memory");
        let cassie_result = Cassie::new_with_data_dir_and_config(&data_dir, config.clone());
        match previous_storage_mode {
            Some(mode) => std::env::set_var("CASSIE_STORAGE_MODE", mode),
            None => std::env::remove_var("CASSIE_STORAGE_MODE"),
        }
        let engine = cassie_result.expect("construct memory-backed Cassie fixture");
        engine
            .startup()
            .expect("start memory-backed Cassie fixture");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind memory-backed pgwire fixture");
        let address = listener.local_addr().expect("read pgwire fixture address");
        drop(listener);
        let server = tokio::spawn(async move {
            let _ = cassie::pgwire::server::run(
                address.to_string(),
                std::sync::Arc::new(engine),
                config,
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        let mut pgwire = tokio_postgres::Config::new();
        pgwire
            .host("127.0.0.1")
            .port(address.port())
            .user("root")
            .password("postgres")
            .dbname("postgres");
        let (client, connection) = pgwire
            .connect(tokio_postgres::NoTls)
            .await
            .expect("connect to memory-backed pgwire fixture");
        let connection = tokio::spawn(async move {
            connection
                .await
                .expect("pgwire connection task should stay healthy");
        });

        // Act
        let dropped = client
            .batch_execute("DROP TABLE IF EXISTS compat_memory_drop_missing")
            .await;
        let created = client
            .batch_execute("CREATE TABLE compat_memory_drop_missing (id INTEGER PRIMARY KEY)")
            .await;
        let inserted = client
            .execute("INSERT INTO compat_memory_drop_missing VALUES (826)", &[])
            .await;
        let drop_sqlstate = dropped
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_string());

        // Assert
        drop(client);
        connection.abort();
        let _ = connection.await;
        server.abort();
        let _ = server.await;
        assert!(
            dropped.is_ok(),
            "DROP TABLE IF EXISTS on a missing relation should succeed; SQLSTATE: {drop_sqlstate:?}"
        );
        assert!(
            created.is_ok(),
            "CREATE TABLE should succeed after the missing-relation drop: {created:?}"
        );
        assert_eq!(
            inserted.expect("insert after CREATE TABLE should succeed"),
            1
        );
    });
}
