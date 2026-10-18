#[test]
fn should_return_connection_backend_pid_without_reusing_another_sessions_result() {
    // Arrange
    use_local_storage();
    let path = data_dir("backend-pid-result-cache");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let setup = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE VIEW backend_pid_view AS SELECT pg_backend_pid() AS pid",
            vec![],
        )
        .expect("create backend pid view");

    runtime().block_on(async {
        let server = pgwire_support::spawn_server(cassie).await;
        let mut first_socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect first client");
        let mut second_socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect second client");
        let (first_read, mut first_writer) = first_socket.split();
        let (second_read, mut second_writer) = second_socket.split();
        let mut first_reader = tokio::io::BufReader::new(first_read);
        let mut second_reader = tokio::io::BufReader::new(second_read);
        let (first_pid, _) =
            pgwire_support::complete_startup_with_backend_key(&mut first_reader, &mut first_writer)
                .await;
        let (second_pid, _) = pgwire_support::complete_startup_with_backend_key(
            &mut second_reader,
            &mut second_writer,
        )
        .await;

        // Act
        let first_direct = write_simple_query_and_read_frames(
            &mut first_reader,
            &mut first_writer,
            "SELECT pg_backend_pid()",
        )
        .await;
        let second_direct = write_simple_query_and_read_frames(
            &mut second_reader,
            &mut second_writer,
            "SELECT pg_backend_pid()",
        )
        .await;
        let first_view = write_simple_query_and_read_frames(
            &mut first_reader,
            &mut first_writer,
            "SELECT pid FROM backend_pid_view",
        )
        .await;
        let second_view = write_simple_query_and_read_frames(
            &mut second_reader,
            &mut second_writer,
            "SELECT pid FROM backend_pid_view",
        )
        .await;

        // Assert
        assert_ne!(first_pid, second_pid);
        assert_eq!(
            pgwire_support::data_rows(&first_direct),
            vec![vec![Some(first_pid.to_string())]]
        );
        assert_eq!(
            pgwire_support::data_rows(&second_direct),
            vec![vec![Some(second_pid.to_string())]]
        );
        assert_eq!(
            pgwire_support::data_rows(&first_view),
            vec![vec![Some(first_pid.to_string())]]
        );
        assert_eq!(
            pgwire_support::data_rows(&second_view),
            vec![vec![Some(second_pid.to_string())]]
        );

        server.stop().await;
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_copy_csv_from_stdin_rows() {
    // Arrange
    use_local_storage();
    let path = data_dir("copy_stdin");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        cassie.startup().unwrap();
        seed_copy_collection(&cassie);

        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr)
            .await
            .expect("connect pgwire");
        let (read_half, mut write_half) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);

        // Act
        start_pgwire_session(&mut reader, &mut write_half).await;
        request_copy_from_stdin(&mut reader, &mut write_half).await;
        let (complete, ready) = send_copy_rows(&mut reader, &mut write_half).await;

        // Assert
        assert_copy_complete_frames(&complete, &ready);
        let select_frames = write_simple_query_and_read_frames(
            &mut reader,
            &mut write_half,
            "SELECT title, score FROM simple_copy_docs ORDER BY score",
        )
        .await;
        assert_copy_select_frames(&select_frames);

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_copy_csv_from_stdin_with_legacy_unparenthesized_options() {
    // Arrange
    use_local_storage();
    let path = data_dir("copy_legacy_csv_stdin");
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.password = "postgres".to_string();
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
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
                &simple_query_frame(
                    "COPY simple_copy_docs (_id, title, score) FROM STDIN WITH CSV HEADER",
                ),
            )
            .await
            .expect("write legacy CSV COPY query");
            tokio::io::AsyncWriteExt::flush(&mut write_half)
                .await
                .expect("flush legacy CSV COPY query");
            let copy_in = read_wire_frame(&mut reader).await;
            let copy_error_code = if copy_in.0 == b'E' {
                error_field(&parse_error_fields(&copy_in.1), 'C').map(str::to_string)
            } else {
                None
            };
            let copied = if copy_in.0 == b'G' {
                let (complete, ready) = send_copy_rows(&mut reader, &mut write_half).await;
                let rows = write_simple_query_and_read_frames(
                    &mut reader,
                    &mut write_half,
                    "SELECT title, score FROM simple_copy_docs ORDER BY score",
                )
                .await;
                Some((complete, ready, rows))
            } else {
                None
            };

            // Clean up before asserting so a syntax rejection does not leave a server task behind.
            drop(socket);
            server.abort();
            let _ = server.await;
            let _ = std::fs::remove_dir_all(path);

            // Assert
            assert_eq!(
                copy_in.0,
                b'G',
                "legacy COPY options should request input; PostgreSQL error SQLSTATE: {copy_error_code:?}"
            );
            let (complete, ready, rows) = copied.expect("COPY input should complete");
            assert_copy_complete_frames(&complete, &ready);
            assert_copy_select_frames(&rows);
        });
}

#[test]
fn should_align_wildcard_data_rows_with_row_description_when_table_declares_id() {
    // Arrange
    use_local_storage();
    let path = data_dir("simple-wildcard-declared-id");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE simple_wildcard_id_docs (id TEXT, title TEXT)",
            "INSERT INTO simple_wildcard_id_docs (id, title) VALUES ('a', 'x')",
            "CREATE TABLE simple_wildcard_upper_id_docs (title TEXT, ID TEXT)",
            "INSERT INTO simple_wildcard_upper_id_docs (title, ID) VALUES ('y', 'b')",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("{sql}: {error}"));
        }
        let (addr, server) = spawn_pgwire_server(&cassie).await;
        let mut socket = tokio::net::TcpStream::connect(addr).await.expect("connect");
        let (read_half, mut writer) = socket.split();
        let mut reader = tokio::io::BufReader::new(read_half);
        start_pgwire_session(&mut reader, &mut writer).await;
        let id_frames = write_simple_query_and_read_frames(
            &mut reader,
            &mut writer,
            "SELECT id FROM simple_wildcard_id_docs",
        )
        .await;
        let row_id = pgwire_support::data_rows(&id_frames)[0][0].clone();

        // Act
        let mut results = Vec::new();
        for sql in [
            "SELECT * FROM simple_wildcard_id_docs",
            "SELECT * FROM simple_wildcard_id_docs WHERE title = 'x'",
            "SELECT * FROM simple_wildcard_id_docs ORDER BY title",
            "SELECT * FROM simple_wildcard_id_docs LIMIT 1",
        ] {
            let frames = write_simple_query_and_read_frames(&mut reader, &mut writer, sql).await;
            results.push((sql, frames));
        }
        let upper_frames = write_simple_query_and_read_frames(
            &mut reader,
            &mut writer,
            "SELECT * FROM simple_wildcard_upper_id_docs",
        )
        .await;

        // Assert
        assert!(row_id.is_some(), "row id should be returned");
        for (sql, frames) in results {
            assert!(
                frames.iter().all(|(tag, _)| *tag != b'E'),
                "{sql} should succeed"
            );
            assert_eq!(
                pgwire_support::row_description_names(&frames),
                vec!["id".to_string(), "title".to_string()],
                "{sql} row description"
            );
            assert_eq!(
                pgwire_support::data_rows(&frames),
                vec![vec![row_id.clone(), Some("x".to_string())]],
                "{sql} data row values"
            );
        }
        // `simple_wildcard_upper_id_docs` declares its own unquoted `ID`
        // field, which folds to `id`, so wildcard output preserves
        // the real schema field order and values instead of
        // synthesizing an internal-identity `id` column.
        assert_eq!(
            pgwire_support::row_description_names(&upper_frames),
            vec!["title".to_string(), "id".to_string()]
        );
        let upper_rows = pgwire_support::data_rows(&upper_frames);
        assert_eq!(upper_rows.len(), 1);
        assert_eq!(upper_rows[0].len(), 2);
        assert_eq!(upper_rows[0][0], Some("y".to_string()));
        assert_eq!(upper_rows[0][1], Some("b".to_string()));

        drop(socket);
        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
