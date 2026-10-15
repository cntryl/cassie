#[test]
fn should_describe_temporal_values_with_postgresql_identities() {
    // Arrange
    let sql = "SELECT DATE '2000-01-02' AS d, TIME '03:04:05.006007' AS t, TIMESTAMP '2000-01-02 03:04:05.006007' AS ts, TIMESTAMPTZ '2000-01-02 03:04:05.006007+00' AS tz, INTERVAL '1 mon 2 days 03:04:05.006007' AS i, TIMETZ '03:04:05.006007+02' AS tztime";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
    ];

    // Act
    let batches = run_script("postgresql18-temporal-identities", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("temporal RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1082, 1083, 1114, 1184, 1186, 1266]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_size)
            .collect::<Vec<_>>(),
        vec![4, 8, 8, 8, 16, 12]
    );
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![
            Some("2000-01-02".to_owned()),
            Some("03:04:05.006007".to_owned()),
            Some("2000-01-02 03:04:05.006007".to_owned()),
            Some("2000-01-02 03:04:05.006007+00".to_owned()),
            Some("1 mon 2 days 03:04:05.006007".to_owned()),
            Some("03:04:05.006007+02".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_temporal_typmods_in_protocol_metadata() {
    // Arrange
    let sql = "SELECT TIME(3) '03:04:05.0067' AS t, TIMESTAMP(2) '2000-01-02 03:04:05.006' AS ts, TIMESTAMPTZ(4) '2000-01-02 03:04:05.00678+00' AS tz, INTERVAL '5.0067 seconds' SECOND(3) AS i";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-temporal-typmods", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "wire error fields: {:?}",
        batches[0]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("temporal typmod RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1083, 1114, 1184, 1186]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_mod)
            .collect::<Vec<_>>(),
        vec![3, 2, 4, 268_435_459]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("03:04:05.007".to_owned()),
            Some("2000-01-02 03:04:05.01".to_owned()),
            Some("2000-01-02 03:04:05.0068+00".to_owned()),
            Some("00:00:05.007".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_interval_field_typmod_in_protocol_metadata() {
    // Arrange
    let sql = "SELECT INTERVAL '5.0067 seconds' SECOND(3) AS i";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-interval-field-typmod", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "wire error fields: {:?}",
        batches[0]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("interval typmod RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!((column.type_oid, column.type_mod), (1186, 268_435_459));
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("00:00:05.007".to_owned())]]
    );
}

#[test]
fn should_describe_temporal_nulls_with_declared_result_types() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT NULL::DATE, NULL::TIME, NULL::TIMESTAMP, NULL::TIMESTAMPTZ, NULL::INTERVAL, NULL::TIMETZ",
    )])];

    // Act
    let batches = run_script("postgresql18-temporal-null-descriptors", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("NULL temporal RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1082, 1083, 1114, 1184, 1186, 1266]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![None, None, None, None, None, None]]
    );
}
