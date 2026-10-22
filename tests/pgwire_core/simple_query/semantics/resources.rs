#[test]
fn should_encode_table_csv_delimiter_null_force_quote_options() {
    // Arrange
    let path = data_dir("copy-table-csv-out");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("Cassie");
    cassie.startup().expect("startup");
    seed_copy_export_rows(&cassie);

    runtime().block_on(async {
        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_pgwire_session(&mut reader, &mut write_half).await;

        // Act
        let frames = write_simple_query_and_read_frames(
            &mut reader,
            &mut write_half,
            "COPY pgwire_copy_export_rows (id, title, note) TO STDOUT \
                 WITH (FORMAT CSV, DELIMITER '|', NULL 'missing', FORCE_QUOTE *)",
        )
        .await;

        // Assert
        assert_copy_out_response(&frames, 0, 3);
        assert_eq!(
            copy_out_bytes(&frames),
            b"\"1\"|\"alpha\"|missing\n\"2\"|\"a,comma\"|\"present\"\n"
        );

        drop(socket);
        server.abort();
        let _ = server.await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_recover_pgwire_after_oversized_sql_resource_limit() {
    // Arrange
    let path = data_dir("sql-resource-limit");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let config = CassieRuntimeConfig {
            password: "postgres".to_string(),
            ..CassieRuntimeConfig::default()
        };
        let cassie = Cassie::new_in_memory_with_config(config.clone()).expect("cassie");
        cassie.startup().expect("startup");
        let (addr, server) = spawn_pgwire_server_with_config(&cassie, config).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_pgwire_session(&mut reader, &mut write_half).await;

        // Act
        let oversized = "x".repeat(1024 * 1024 + 1);
        let rejected =
            write_simple_query_and_read_frames(&mut reader, &mut write_half, &oversized).await;
        let recovered =
            write_simple_query_and_read_frames(&mut reader, &mut write_half, "SELECT version()")
                .await;

        // Assert
        assert_eq!(rejected[0].0, b'E');
        assert_eq!(
            error_field(&parse_error_fields(&rejected[0].1), 'C'),
            Some("54000")
        );
        assert_eq!(rejected.last().map(|frame| frame.0), Some(b'Z'));
        assert!(recovered.iter().all(|frame| frame.0 != b'E'));
        assert_eq!(recovered.last().map(|frame| frame.0), Some(b'Z'));

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
