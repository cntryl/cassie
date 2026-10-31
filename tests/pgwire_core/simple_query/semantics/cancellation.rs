#[test]
fn should_report_deadline_exceeded_with_query_canceled_sqlstate() {
    // Arrange
    let path = data_dir("query_deadline");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        crate::support_sql::test_root_password().clone_into(&mut config.password);
        config.limits.query_timeout_ms = 1;
        let cassie = Cassie::new_in_memory_with_config(config.clone()).unwrap();
        cassie.startup().expect("startup");
        seed_simple_query_collection(&cassie);
        let (addr, server) = spawn_pgwire_server_with_config(&cassie, config).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        let sql = format!(
            "{}SELECT title FROM simple_query_docs",
            " ".repeat(1_000_000)
        );
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &simple_query_frame(&sql))
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
        assert_eq!(
            error_fields
                .iter()
                .find(|(field, _)| *field == 'C')
                .map(|(_, value)| value.as_str()),
            Some("57014"),
            "unexpected pgwire error fields: {error_fields:?}",
        );
        assert!(
            error_fields.iter().any(|(field, value)| {
                *field == 'M' && value.contains("query timeout exceeded")
            }),
            "deadline error should mention query timeout"
        );

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
