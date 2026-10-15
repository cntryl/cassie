#[test]
fn should_encode_binary_copy_with_exact_int4_null_framing() {
    // Arrange
    use_local_storage();
    let path = data_dir("copy-table-binary-out");
    let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
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
            "COPY pgwire_copy_export_rows (id, title, note) TO STDOUT WITH (FORMAT BINARY)",
        )
        .await;

        // Assert
        assert_copy_out_response(&frames, 1, 3);
        let mut expected = b"PGCOPY\n\xff\r\n\0".to_vec();
        expected.extend_from_slice(&0_i32.to_be_bytes());
        expected.extend_from_slice(&0_i32.to_be_bytes());
        expected.extend_from_slice(&3_i16.to_be_bytes());
        expected.extend_from_slice(&4_i32.to_be_bytes());
        expected.extend_from_slice(&1_i32.to_be_bytes());
        expected.extend_from_slice(&5_i32.to_be_bytes());
        expected.extend_from_slice(b"alpha");
        expected.extend_from_slice(&(-1_i32).to_be_bytes());
        expected.extend_from_slice(&3_i16.to_be_bytes());
        expected.extend_from_slice(&4_i32.to_be_bytes());
        expected.extend_from_slice(&2_i32.to_be_bytes());
        expected.extend_from_slice(&7_i32.to_be_bytes());
        expected.extend_from_slice(b"a,comma");
        expected.extend_from_slice(&7_i32.to_be_bytes());
        expected.extend_from_slice(b"present");
        expected.extend_from_slice(&(-1_i16).to_be_bytes());
        assert_eq!(copy_out_bytes(&frames), expected);

        drop(socket);
        server.abort();
        let _ = server.await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_execute_binary_simple_query_return_backend_frames() {
    // Arrange
    use_local_storage();
    let path = data_dir("success");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        seed_simple_query_collection(&cassie);

        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        let frames = write_simple_query_and_read_frames(
            &mut reader,
            &mut write_half,
            "SELECT title FROM simple_query_docs ORDER BY title",
        )
        .await;

        // Assert
        assert_simple_query_backend_frames(&frames);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
