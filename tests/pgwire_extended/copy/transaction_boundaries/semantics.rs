#[test]
fn should_stage_table_copy_until_explicit_commit() {
    // Arrange
    let boundary = copy_cycle();
    let tail = vec![vec![wire::simple_query_frame("COMMIT")]];

    // Act
    let result = run_script(
        Prefix::Explicit,
        boundary,
        "SELECT n FROM wire_tx ORDER BY n",
        tail,
    );

    // Assert
    assert_eq!(wire::error_code(&result.boundary), None);
    assert_eq!(
        result
            .boundary
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"GCZ"
    );
    assert_eq!(
        result.boundary.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    assert_eq!(
        wire::data_rows(&result.after),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(
        wire::data_rows(&result.terminal),
        vec![vec![Some("1".to_string())], vec![Some("2".to_string())]]
    );
}

fn staged_copy_binary_row() -> Vec<u8> {
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&2_i16.to_be_bytes());
    payload.extend_from_slice(&4_i32.to_be_bytes());
    payload.extend_from_slice(&41_i32.to_be_bytes());
    payload.extend_from_slice(&5_i32.to_be_bytes());
    payload.extend_from_slice(b"alpha");
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    payload
}

#[test]
fn should_recover_jsonb_binary_copy_error_in_implicit_and_explicit_transactions() {
    // Arrange
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&5_i32.to_be_bytes());
    payload.extend_from_slice(b"\x01[1,]");
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    for explicit in [false, true] {
        let mut script = Vec::new();
        if explicit {
            script.push(vec![wire::simple_query_frame("BEGIN")]);
            script.push(vec![wire::simple_query_frame("SAVEPOINT before_copy")]);
        }
        let copy_index = script.len();
        script.push(vec![
            wire::simple_query_frame(
                "COPY jsonb_copy_recovery (value) FROM STDIN WITH (FORMAT BINARY)",
            ),
            wire::copy_data_frame(&payload),
            wire::copy_done_frame(),
        ]);
        if explicit {
            script.push(vec![wire::simple_query_frame("ROLLBACK TO before_copy")]);
        }
        script.push(vec![wire::simple_query_frame(
            "SELECT count(*) FROM jsonb_copy_recovery",
        )]);
        // Act
        let frames = crate::support_repeatable_read::run_own_script(
            &["CREATE TABLE jsonb_copy_recovery (value JSONB)"],
            script,
        );
        // Assert
        assert_eq!(
            wire::error_code(&frames[copy_index]).as_deref(),
            Some("42601")
        );
        assert_eq!(
            frames[copy_index].last().map(|frame| frame.1.as_slice()),
            Some(if explicit { &b"E"[..] } else { &b"I"[..] })
        );
        assert_eq!(
            wire::data_rows(frames.last().expect("recovery")),
            vec![vec![Some("0".into())]]
        );
    }
}

#[test]
fn should_recover_jsonb_array_binary_copy_error_in_implicit_and_explicit_transactions() {
    // Arrange
    let mut array = Vec::new();
    for value in [1_i32, 0, 3802, 1, 1, 5] {
        array.extend_from_slice(&value.to_be_bytes());
    }
    array.extend_from_slice(b"\x01[1,]");
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&i32::try_from(array.len()).expect("length").to_be_bytes());
    payload.extend_from_slice(&array);
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    for explicit in [false, true] {
        let mut script = Vec::new();
        if explicit {
            script.push(vec![wire::simple_query_frame("BEGIN")]);
            script.push(vec![wire::simple_query_frame("SAVEPOINT before_copy")]);
        }
        let copy_index = script.len();
        script.push(vec![
            wire::simple_query_frame(
                "COPY jsonb_array_copy_recovery (value) FROM STDIN WITH (FORMAT BINARY)",
            ),
            wire::copy_data_frame(&payload),
            wire::copy_done_frame(),
        ]);
        if explicit {
            script.push(vec![wire::simple_query_frame("ROLLBACK TO before_copy")]);
        }
        script.push(vec![wire::simple_query_frame(
            "SELECT count(*) FROM jsonb_array_copy_recovery",
        )]);
        // Act
        let frames = crate::support_repeatable_read::run_own_script(
            &["CREATE TABLE jsonb_array_copy_recovery (value JSONB[])"],
            script,
        );
        // Assert
        assert_eq!(
            wire::error_code(&frames[copy_index]).as_deref(),
            Some("42601")
        );
        assert_eq!(
            frames[copy_index].last().map(|frame| frame.1.as_slice()),
            Some(if explicit { &b"E"[..] } else { &b"I"[..] })
        );
        assert_eq!(
            wire::data_rows(frames.last().expect("recovery")),
            vec![vec![Some("0".into())]]
        );
    }
}

#[test]
fn should_copy_into_transaction_owned_table_across_wire_formats() {
    // Arrange
    let formats = [
        ("TEXT", b"41\talpha\n".to_vec()),
        ("CSV", b"41,alpha\n".to_vec()),
        ("BINARY", staged_copy_binary_row()),
    ];
    for (format, payload) in formats {
        let copy = format!("COPY staged_copy_wire (n, payload) FROM STDIN WITH (FORMAT {format})");
        let script = vec![
            vec![wire::simple_query_frame("BEGIN")],
            vec![wire::simple_query_frame(
                "CREATE TABLE staged_copy_wire (id SERIAL PRIMARY KEY, n INT, payload TEXT)",
            )],
            vec![
                wire::simple_query_frame(&copy),
                wire::copy_data_frame(&payload),
                wire::copy_done_frame(),
            ],
            vec![wire::simple_query_frame(
                "SELECT id,n,payload FROM staged_copy_wire",
            )],
            vec![wire::simple_query_frame(
                "SELECT currval('staged_copy_wire_id_seq'), lastval()",
            )],
            vec![wire::simple_query_frame("COMMIT")],
            vec![wire::simple_query_frame(
                "SELECT nextval('staged_copy_wire_id_seq')",
            )],
        ];

        // Act
        let frames = crate::support_repeatable_read::run_own_script(&[], script);

        // Assert
        assert_eq!(
            wire::error_code(&frames[2]),
            None,
            "{format}: {:?}",
            frames[2]
        );
        assert!(frames[2].iter().any(|frame| frame.0 == b'G'));
        assert_eq!(
            wire::data_rows(&frames[3]),
            vec![vec![
                Some("1".into()),
                Some("41".into()),
                Some("alpha".into())
            ]]
        );
        assert_eq!(
            wire::data_rows(&frames[4]),
            vec![vec![Some("1".into()), Some("1".into())]]
        );
        assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("2".into())]]);
    }
}

#[test]
fn should_copy_into_a_transaction_recreated_target_and_restore_the_original() {
    // Arrange
    let script = vec![
        vec![wire::simple_query_frame("BEGIN")],
        vec![wire::simple_query_frame("DROP TABLE staged_copy_recreated")],
        vec![wire::simple_query_frame(
            "CREATE TABLE staged_copy_recreated (payload TEXT)",
        )],
        vec![
            wire::simple_query_frame("COPY staged_copy_recreated (payload) FROM STDIN"),
            wire::copy_data_frame(b"new\n"),
            wire::copy_done_frame(),
        ],
        vec![wire::simple_query_frame(
            "SELECT payload FROM staged_copy_recreated",
        )],
        vec![wire::simple_query_frame("ROLLBACK")],
        vec![wire::simple_query_frame(
            "SELECT id, marker FROM staged_copy_recreated",
        )],
    ];

    // Act
    let frames = crate::support_repeatable_read::run_own_script(
        &[
            "CREATE TABLE staged_copy_recreated (id INT, marker TEXT)",
            "INSERT INTO staged_copy_recreated VALUES (7, 'original')",
        ],
        script,
    );

    // Assert
    assert_eq!(wire::error_code(&frames[3]), None);
    assert_eq!(wire::data_rows(&frames[4]), vec![vec![Some("new".into())]]);
    assert_eq!(
        wire::data_rows(&frames[6]),
        vec![vec![Some("7".into()), Some("original".into())]]
    );
}

#[test]
fn should_rollback_a_staged_copy_error_without_rewinding_its_sequence() {
    // Arrange
    let script = vec![
        vec![wire::simple_query_frame("BEGIN")],
        vec![wire::simple_query_frame(
            "CREATE TABLE staged_copy_error (id SERIAL PRIMARY KEY, n INT)",
        )],
        vec![wire::simple_query_frame("SAVEPOINT before_copy")],
        vec![
            wire::simple_query_frame("COPY staged_copy_error (n) FROM STDIN"),
            wire::copy_data_frame(b"1\nnot_integer\n"),
            wire::copy_done_frame(),
        ],
        vec![wire::simple_query_frame("ROLLBACK TO before_copy")],
        vec![wire::simple_query_frame(
            "SELECT count(*) FROM staged_copy_error",
        )],
        vec![wire::simple_query_frame(
            "SELECT nextval('staged_copy_error_id_seq')",
        )],
        vec![wire::simple_query_frame("ROLLBACK")],
    ];

    // Act
    let frames = crate::support_repeatable_read::run_own_script(&[], script);

    // Assert
    assert_eq!(wire::error_code(&frames[3]).as_deref(), Some("22P02"));
    assert_eq!(wire::data_rows(&frames[5]), vec![vec![Some("0".into())]]);
    assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("2".into())]]);
}

#[test]
fn should_copy_into_an_unchanged_published_table_after_unrelated_ddl() {
    // Arrange
    let script = vec![
        vec![wire::simple_query_frame("BEGIN")],
        vec![wire::simple_query_frame(
            "CREATE TABLE staged_copy_unrelated (v INT)",
        )],
        vec![
            wire::simple_query_frame("COPY staged_copy_published (n) FROM STDIN"),
            wire::copy_data_frame(b"41\n"),
            wire::copy_done_frame(),
        ],
        vec![wire::simple_query_frame("COMMIT")],
        vec![wire::simple_query_frame(
            "SELECT n FROM staged_copy_published",
        )],
    ];

    // Act
    let frames = crate::support_repeatable_read::run_own_script(
        &["CREATE TABLE staged_copy_published (n INT)"],
        script,
    );

    // Assert
    assert_eq!(wire::error_code(&frames[2]), None);
    assert_eq!(wire::data_rows(&frames[4]), vec![vec![Some("41".into())]]);
}

#[test]
fn should_publish_staged_copy_rows_and_catalog_together_to_other_sessions() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let (before, after) = runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("memory fixture");
        cassie.startup().expect("startup");
        let server = wire::spawn_server(cassie).await;
        let owner = tokio::net::TcpStream::connect(server.addr).await.expect("owner");
        let observer = tokio::net::TcpStream::connect(server.addr).await.expect("observer");
        let (mut owner_read, mut owner_write) = owner.into_split();
        let (mut observer_read, mut observer_write) = observer.into_split();
        wire::complete_startup(&mut owner_read, &mut owner_write).await;
        wire::complete_startup(&mut observer_read, &mut observer_write).await;
        for sql in ["BEGIN", "CREATE TABLE staged_copy_visibility (id INT)"] {
            wire::write_frames(&mut owner_write, vec![wire::simple_query_frame(sql)]).await;
            let frames = wire::read_frames_until_ready_within(&mut owner_read, std::time::Duration::from_secs(5)).await;
            assert_eq!(wire::error_code(&frames), None);
        }
        wire::write_frames(&mut owner_write, vec![wire::simple_query_frame("COPY staged_copy_visibility FROM STDIN"), wire::copy_data_frame(b"41\n"), wire::copy_done_frame()]).await;
        let copy = wire::read_frames_until_ready_within(&mut owner_read, std::time::Duration::from_secs(5)).await;
        assert_eq!(wire::error_code(&copy), None);

        // Act
        let probe = "SELECT count(*) FROM information_schema.tables WHERE table_name = 'staged_copy_visibility'";
        wire::write_frames(&mut observer_write, vec![wire::simple_query_frame(probe)]).await;
        let before = wire::read_frames_until_ready_within(&mut observer_read, std::time::Duration::from_secs(5)).await;
        wire::write_frames(&mut owner_write, vec![wire::simple_query_frame("COMMIT")]).await;
        let commit = wire::read_frames_until_ready_within(&mut owner_read, std::time::Duration::from_secs(5)).await;
        assert_eq!(wire::error_code(&commit), None);
        wire::write_frames(&mut observer_write, vec![wire::simple_query_frame("SELECT id FROM staged_copy_visibility")]).await;
        let after = wire::read_frames_until_ready_within(&mut observer_read, std::time::Duration::from_secs(5)).await;
        drop((owner_read, owner_write, observer_read, observer_write));
        server.stop().await;
        (before, after)
    });

    // Assert
    assert_eq!(wire::data_rows(&before), vec![vec![Some("0".into())]]);
    assert_eq!(wire::data_rows(&after), vec![vec![Some("41".into())]]);
}

#[test]
fn should_preserve_standalone_autocommit_copy_without_an_owner() {
    // Arrange
    let script = vec![
        vec![
            wire::simple_query_frame("COPY standalone_copy_owner (n) FROM STDIN"),
            wire::copy_data_frame(b"41\n"),
            wire::copy_done_frame(),
        ],
        vec![wire::simple_query_frame(
            "SELECT n FROM standalone_copy_owner",
        )],
    ];

    // Act
    let frames = crate::support_repeatable_read::run_own_script(
        &["CREATE TABLE standalone_copy_owner (n INT)"],
        script,
    );

    // Assert
    assert_eq!(wire::error_code(&frames[0]), None);
    assert_eq!(wire::data_rows(&frames[1]), vec![vec![Some("41".into())]]);
}

#[test]
fn should_copy_to_staged_search_path_schema_instead_of_public_collision() {
    // Arrange
    let script = vec![
        vec![wire::simple_query_frame(
            "CREATE TABLE public.copy_path_target (n INT)",
        )],
        vec![wire::simple_query_frame("BEGIN")],
        vec![wire::simple_query_frame("CREATE SCHEMA staged_copy_path")],
        vec![wire::simple_query_frame(
            "SET search_path = staged_copy_path",
        )],
        vec![wire::simple_query_frame(
            "CREATE TABLE staged_copy_path.copy_path_target (n INT)",
        )],
        vec![
            wire::simple_query_frame("COPY copy_path_target FROM STDIN"),
            wire::copy_data_frame(b"7\n"),
            wire::copy_done_frame(),
        ],
        vec![wire::simple_query_frame(
            "SELECT n FROM staged_copy_path.copy_path_target",
        )],
        vec![wire::simple_query_frame(
            "SELECT n FROM public.copy_path_target",
        )],
        vec![wire::simple_query_frame("ROLLBACK")],
    ];
    // Act
    let frames = crate::support_repeatable_read::run_own_script(&[], script);
    // Assert
    assert_eq!(wire::error_code(&frames[5]), None);
    assert_eq!(wire::data_rows(&frames[6]), vec![vec![Some("7".into())]]);
    assert!(wire::data_rows(&frames[7]).is_empty());
}

#[test]
fn should_report_unknown_quoted_copy_where_column_and_recover() {
    // Arrange
    let script = vec![
        vec![wire::simple_query_frame(
            "CREATE TABLE copy_unknown_where (id INT)",
        )],
        vec![
            wire::simple_query_frame(
                "COPY copy_unknown_where FROM STDIN WHERE (\"missing_é\" = 1)",
            ),
            wire::copy_data_frame(b"1\n"),
            wire::copy_done_frame(),
        ],
        vec![wire::simple_query_frame("SELECT 1")],
    ];
    // Act
    let frames = crate::support_repeatable_read::run_own_script(&[], script);
    // Assert
    assert_eq!(wire::error_code(&frames[1]).as_deref(), Some("42703"));
    assert_eq!(wire::error_code(&frames[2]), None);
    assert_eq!(wire::data_rows(&frames[2]), vec![vec![Some("1".into())]]);
}

#[test]
fn should_recover_binary_timestamp_range_errors_in_implicit_and_explicit_transactions() {
    // Arrange
    use cassie::types::extended::temporal::calendar::END_TIMESTAMP;
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&8_i32.to_be_bytes());
    payload.extend_from_slice(&END_TIMESTAMP.to_be_bytes());
    payload.extend_from_slice(&(-1_i16).to_be_bytes());
    for sql_type in ["TIMESTAMP", "TIMESTAMPTZ"] {
        for explicit in [false, true] {
            let mut script = Vec::new();
            if explicit {
                script.push(vec![wire::simple_query_frame("BEGIN")]);
                script.push(vec![wire::simple_query_frame("SAVEPOINT before_copy")]);
            }
            let copy_index = script.len();
            script.push(vec![
                wire::simple_query_frame(
                    "COPY timestamp_copy_bounds (value) FROM STDIN WITH (FORMAT BINARY)",
                ),
                wire::copy_data_frame(&payload),
                wire::copy_done_frame(),
            ]);
            if explicit {
                script.push(vec![wire::simple_query_frame("ROLLBACK TO before_copy")]);
            }
            script.push(vec![wire::simple_query_frame(
                "SELECT count(*) FROM timestamp_copy_bounds",
            )]);

            // Act
            let setup = format!("CREATE TABLE timestamp_copy_bounds (value {sql_type})");
            let frames = crate::support_repeatable_read::run_own_script(&[&setup], script);

            // Assert
            assert_eq!(
                wire::error_code(&frames[copy_index]).as_deref(),
                Some("22008")
            );
            assert_eq!(
                frames[copy_index].last().map(|frame| frame.1.as_slice()),
                Some(if explicit { &b"E"[..] } else { &b"I"[..] })
            );
            assert_eq!(
                wire::data_rows(frames.last().expect("recovery")),
                vec![vec![Some("0".into())]]
            );
        }
    }
}
