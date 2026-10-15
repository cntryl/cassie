#[test]
fn should_recover_ready_after_simple_query_error() {
    // Arrange
    use_local_storage();
    let path = data_dir("error");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            std::sync::Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &simple_query_frame("SELECT title FROM missing_simple_query_table"),
        )
        .await
        .expect("write query");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush query");

        let error = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;

        // Assert
        assert_eq!(error.0, b'E', "query failure should return an error frame");
        assert_eq!(
            ready.0, b'Z',
            "query failure should still return ready-for-query"
        );
        let error_fields = parse_error_fields(&error.1);
        assert_eq!(
            error_fields
                .iter()
                .find(|(field, _)| *field == 'C')
                .map(|(_, value)| value.as_str()),
            Some("42P01"),
            "missing table should use undefined table SQLSTATE"
        );
        assert_eq!(
            error_fields
                .iter()
                .find(|(field, _)| *field == 't')
                .map(|(_, value)| value.as_str()),
            Some("missing_simple_query_table"),
            "missing table should include table metadata"
        );
        assert!(
            error_fields.iter().any(|(field, value)| {
                *field == 'M' && value.contains("missing_simple_query_table")
            }),
            "error response should mention the missing table"
        );
        assert_eq!(ready.1, vec![b'I']);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_retryable_storage_error_with_cannot_connect_now_sqlstate() {
    // Arrange
    use_local_storage();
    let path = data_dir("retryable_storage");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        cassie::pgwire::connection::arm_next_pgwire_blocking_retryable_failure_for_test(
            &cassie,
            "pgwire blocking boundary test retryable failure",
        );
        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &simple_query_frame("SELECT version()"),
        )
        .await
        .expect("write query");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush query");
        let error = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;
        let error_fields = parse_error_fields(&error.1);

        // Assert
        assert_eq!(error.0, b'E');
        assert_eq!(ready.0, b'Z');
        assert_eq!(error_field(&error_fields, 'C'), Some("57P03"));
        assert_eq!(
            error_field(&error_fields, 'M'),
            Some("temporary storage unavailable: pgwire blocking boundary test retryable failure")
        );
        assert_eq!(ready.1, vec![b'I']);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_bigint_out_of_range_with_sqlstate_22003() {
    // Arrange
    use_local_storage();
    let path = data_dir("bigint_out_of_range");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &simple_query_frame("SELECT 9223372036854775807 + 1"),
        )
        .await
        .expect("write query");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush query");
        let error = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;
        let error_fields = parse_error_fields(&error.1);

        // Assert
        assert_eq!(error.0, b'E');
        assert_eq!(ready.0, b'Z');
        assert_eq!(error_field(&error_fields, 'C'), Some("22003"));
        assert_eq!(error_field(&error_fields, 'M'), Some("bigint out of range"));

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_division_by_zero_with_sqlstate_22012() {
    // Arrange
    use_local_storage();
    let path = data_dir("division_by_zero");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE division_by_zero (score INT)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO division_by_zero (score) VALUES (4)",
                vec![],
            )
            .expect("seed row");
        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &simple_query_frame("SELECT score FROM division_by_zero WHERE (score / 0) = 1"),
        )
        .await
        .expect("write query");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush query");
        let error = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;
        let error_fields = parse_error_fields(&error.1);

        // Assert
        assert_eq!(error.0, b'E');
        assert_eq!(ready.0, b'Z');
        assert_eq!(error_field(&error_fields, 'C'), Some("22012"));
        assert_eq!(error_field(&error_fields, 'M'), Some("division by zero"));
        assert_eq!(ready.1, vec![b'I']);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
