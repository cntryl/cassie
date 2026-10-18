#[test]
fn should_expand_multiple_unnest_arrays_in_lockstep_with_ordinality() {
    // Arrange
    let sql = "SELECT first_value, second_value, ordinal FROM unnest(ARRAY[10,20,30], ARRAY['a','b']) WITH ORDINALITY AS rows(first_value, second_value, ordinal) ORDER BY ordinal";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-unnest-lockstep-ordinality", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("UNNEST WITH ORDINALITY RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25, 20]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("10".to_owned()),
                Some("a".to_owned()),
                Some("1".to_owned())
            ],
            vec![
                Some("20".to_owned()),
                Some("b".to_owned()),
                Some("2".to_owned())
            ],
            vec![Some("30".to_owned()), None, Some("3".to_owned())],
        ]
    );
}

#[test]
fn should_expand_integer_generate_series_with_a_nonunit_step() {
    // Arrange
    let sql = "SELECT value FROM generate_series(2, 8, 3) AS series(value)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-generate-series-step", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("generate_series RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("2".to_owned())],
            vec![Some("5".to_owned())],
            vec![Some("8".to_owned())],
        ]
    );
}

#[test]
fn should_generate_array_subscripts_in_reverse_from_the_declared_lower_bound() {
    // Arrange
    let sql = "SELECT subscript FROM generate_subscripts('[0:2]={10,20,30}'::INT[], 1, TRUE) AS indexes(subscript)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-generate-subscripts-reverse", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("generate_subscripts RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("2".to_owned())],
            vec![Some("1".to_owned())],
            vec![Some("0".to_owned())],
        ]
    );
}

#[test]
fn should_zip_rows_from_functions_with_ordinality() {
    // Arrange
    let sql = "SELECT * FROM ROWS FROM (generate_series(1, 3), generate_series(10, 20, 10)) WITH ORDINALITY AS rows(first_value, second_value, ordinal)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-rows-from-ordinality", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("ROWS FROM RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 20]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("1".to_owned()),
                Some("10".to_owned()),
                Some("1".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("20".to_owned()),
                Some("2".to_owned()),
            ],
            vec![Some("3".to_owned()), None, Some("3".to_owned())],
        ]
    );
}

#[test]
fn should_zip_select_list_set_returning_functions_to_the_longest_result() {
    // Arrange
    let sql =
        "SELECT generate_series(1, 3) AS first_value, generate_series(10, 20, 10) AS second_value";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-select-list-srf-lockstep", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("select-list SRF RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
            vec![Some("3".to_owned()), None],
        ]
    );
}

#[test]
fn should_execute_scalar_function_in_from_as_one_row_relation() {
    // Arrange
    let sql = "SELECT lowered FROM lower('HELLO') AS scalar_function(lowered)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-scalar-function-from", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("scalar function in FROM RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("hello".to_owned())]]
    );
}

#[test]
fn should_expand_numeric_generate_series_with_exact_steps() {
    // Arrange
    let sql = "SELECT generate_series(0.25, 1.0, 0.3)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-numeric-generate-series", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric generate_series RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1700
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("0.25".to_owned())],
            vec![Some("0.55".to_owned())],
            vec![Some("0.85".to_owned())],
        ]
    );
}

#[test]
fn should_advance_timestamptz_generate_series_by_local_days_across_dst() {
    // Arrange
    let sql = "SELECT generate_series(\
                  '2024-03-09 12:00:00-05', \
                  '2024-03-10 12:00:00-04', \
                  '1 day', 'America/New_York'\
               ) = '2024-03-10 12:00:00-04' OFFSET 1 LIMIT 1";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-808-timestamptz-series-timezone", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("time zone aware generate_series RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_bound_set_returning_results_without_publishing_partial_rows() {
    // Arrange
    let (storage, config) = configured_fixture("issue-808-srf-result-cap", 2);
    let runtime = current_thread_runtime();

    // Act
    let (overflow, bounded, recovery) = runtime.block_on(async {
        let (address, server) = start_server(storage.cassie.clone(), config).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        let overflow_queries = [
            "SELECT value FROM unnest(ARRAY[1,2,3]) AS expanded(value)",
            "SELECT * FROM ROWS FROM (generate_series(1,3), generate_series(10,30,10)) AS expanded(first_value, second_value)",
            "SELECT value FROM generate_series(1,3) AS expanded(value)",
        ];
        let mut overflow = Vec::with_capacity(overflow_queries.len());
        for query in overflow_queries {
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(query)]).await;
            overflow.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }

        let bounded_queries = [
            "SELECT value FROM unnest(ARRAY[1,2,3]) AS expanded(value) LIMIT 2",
            "SELECT * FROM ROWS FROM (generate_series(1,3), generate_series(10,30,10)) AS expanded(first_value, second_value) LIMIT 2",
            "SELECT value FROM generate_series(1,10000) AS expanded(value) LIMIT 2",
        ];
        let mut bounded = Vec::with_capacity(bounded_queries.len());
        for query in bounded_queries {
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(query)]).await;
            bounded.push(
                wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await,
            );
        }

        wire::write_frames(&mut writer, vec![wire::simple_query_frame("SELECT 42")]).await;
        let recovery =
            wire::read_frames_until_ready_within(&mut reader, Duration::from_secs(5)).await;

        drop(reader);
        drop(writer);
        server.abort();
        let _ = server.await;
        (overflow, bounded, recovery)
    });
    drop(runtime);
    drop(storage);

    // Assert
    assert_eq!(overflow.len(), 3);
    for frames in &overflow {
        assert_eq!(wire::error_code(frames).as_deref(), Some("54000"));
        assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
        assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
        let error = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("result-cap ErrorResponse");
        let fields = wire::parse_error_fields(&error.1);
        assert_eq!(
            fields
                .iter()
                .find(|(field, _)| *field == 'M')
                .map(|(_, message)| message.as_str()),
            Some("query result row limit exceeded: 3 > 2")
        );
    }

    let expected_bounded = [
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]],
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
        ],
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]],
    ];
    assert_eq!(bounded.len(), expected_bounded.len());
    for (frames, rows) in bounded.iter().zip(expected_bounded) {
        assert_eq!(wire::error_code(frames), None);
        assert_eq!(wire::data_rows(frames), rows);
    }
    assert_eq!(wire::error_code(&recovery), None);
    assert_eq!(
        wire::data_rows(&recovery),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_use_explicit_timezone_for_timestamptz_series_across_dst() {
    // Arrange
    let query = "SELECT value FROM generate_series(\
        '2024-03-09 12:00:00-05'::TIMESTAMPTZ, \
        '2024-03-12 12:00:00-04'::TIMESTAMPTZ, \
        INTERVAL '1 day', 'America/New_York') AS series(value)";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(query)]),
    ];

    // Act
    let batches = run_script("issue-808-generate-series-explicit-timezone-dst", steps);

    // Assert
    let result = &batches[1];
    assert_eq!(
        wire::error_code(result),
        None,
        "wire errors: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("four-argument generate_series RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        1_184
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("2024-03-09 17:00:00+00".to_owned())],
            vec![Some("2024-03-10 16:00:00+00".to_owned())],
            vec![Some("2024-03-11 16:00:00+00".to_owned())],
            vec![Some("2024-03-12 16:00:00+00".to_owned())],
        ]
    );
}
