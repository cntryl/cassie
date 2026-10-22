#[test]
fn should_copy_default_text_from_stdin_with_null_values_tab_escapes() {
    // Arrange
    let path = data_dir("copy_text_stdin");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.password = "postgres".to_string();
    let cassie = Cassie::new_in_memory_with_config(config).expect("cassie");
    cassie.startup().expect("startup");
    seed_copy_collection(&cassie);

    runtime().block_on(async {
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
            &simple_query_frame("COPY simple_copy_docs (_id, title, score) FROM STDIN"),
        )
        .await
        .expect("write default-text COPY query");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush COPY query");
        let copy_in = read_wire_frame(&mut reader).await;
        assert_eq!(copy_in.0, b'G', "COPY should request input data");
        assert_eq!(copy_in.1[0], 0, "COPY without FORMAT uses text format");
        assert_eq!(
            i16::from_be_bytes(copy_in.1[1..3].try_into().expect("COPY column count")),
            3
        );
        let payload = b"copy-text-1\talpha\t\\N\ncopy-text-2\tline\\tdetail\t9\n";
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &copy_data_frame(payload))
            .await
            .expect("write default-text COPY rows");
        tokio::io::AsyncWriteExt::write_all(&mut write_half, &copy_done_frame())
            .await
            .expect("finish COPY input");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush COPY rows");
        let complete = read_wire_frame(&mut reader).await;
        let ready = read_wire_frame(&mut reader).await;
        let rows = write_simple_query_and_read_frames(
            &mut reader,
            &mut write_half,
            "SELECT _id, title, score FROM simple_copy_docs ORDER BY _id",
        )
        .await;

        // Assert
        assert_copy_complete_frames(&complete, &ready);
        assert_eq!(
            rows.iter()
                .filter(|(tag, _)| *tag == b'D')
                .map(|(_, payload)| parse_data_row(payload))
                .collect::<Vec<_>>(),
            vec![
                vec![
                    Some("copy-text-1".to_string()),
                    Some("alpha".to_string()),
                    None,
                ],
                vec![
                    Some("copy-text-2".to_string()),
                    Some("line\tdetail".to_string()),
                    Some("9".to_string()),
                ],
            ]
        );

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_encode_query_csv_header_null_force_quote_options() {
    // Arrange
    let path = data_dir("copy-query-csv-out");
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
            "COPY (SELECT id, title, note FROM pgwire_copy_export_rows ORDER BY id) \
                 TO STDOUT WITH (FORMAT CSV, HEADER, NULL 'NULL', FORCE_QUOTE (title))",
        )
        .await;

        // Assert
        assert_copy_out_response(&frames, 0, 3);
        assert_eq!(
            copy_out_bytes(&frames),
            b"id,title,note\n1,\"alpha\",NULL\n2,\"a,comma\",present\n"
        );

        drop(socket);
        server.abort();
        let _ = server.await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_encode_query_default_text_copy_with_null_fields() {
    // Arrange
    let path = data_dir("copy-query-text-out");
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
            "COPY (SELECT id, title, note FROM pgwire_copy_export_rows ORDER BY id) \
                 TO STDOUT",
        )
        .await;

        // Assert
        assert_copy_out_response(&frames, 0, 3);
        assert_eq!(
            copy_out_bytes(&frames),
            b"1\talpha\t\\N\n2\ta,comma\tpresent\n"
        );

        drop(socket);
        server.abort();
        let _ = server.await;
    });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_return_row_description_for_empty_simple_query_result() {
    // Arrange
    let path = data_dir("empty_result");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_in_memory_with_config(config.clone()).unwrap();
        cassie.startup().unwrap();

        let collection = canonical_relation_name("postgres", "public", "simple_query_empty_docs");
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };
        cassie
            .midge
            .create_collection(&collection, schema.clone())
            .unwrap();
        cassie.register_collection(&collection, schema);

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

        start_pgwire_session(&mut reader, &mut write_half).await;

        // Act
        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            &simple_query_frame(
                "SELECT title FROM simple_query_empty_docs WHERE title = 'missing'",
            ),
        )
        .await
        .expect("write query");
        tokio::io::AsyncWriteExt::flush(&mut write_half)
            .await
            .expect("flush query");

        let mut frames = Vec::new();
        loop {
            let frame = read_wire_frame(&mut reader).await;
            let tag = frame.0;
            frames.push(frame);
            if tag == b'Z' {
                break;
            }
        }

        // Assert
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].0, b'T', "empty select should describe columns");
        assert_eq!(frames[1].0, b'C', "empty select should complete command");
        assert_eq!(frames[2].0, b'Z', "empty select should return ready");
        assert_eq!(parse_row_description(&frames[0].1)[0].0, "title");
        assert_eq!(frames[2].1, vec![b'I']);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
