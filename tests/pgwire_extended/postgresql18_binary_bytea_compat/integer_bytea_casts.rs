use super::*;

#[test]
fn should_round_trip_signed_integer_boundaries_through_bytea() {
    // Arrange
    let sql = "SELECT (-32768)::INT2::BYTEA AS small_bytes, \
                      2147483647::INT4::BYTEA AS integer_bytes, \
                      (-9223372036854775808)::INT8::BYTEA AS bigint_bytes, \
                      decode('8000', 'hex')::INT2 AS small_value, \
                      decode('7fffffff', 'hex')::INT4 AS integer_value, \
                      decode('8000000000000000', 'hex')::INT8 AS bigint_value";
    let batches = run_script(
        "issue-816-integer-bytea-cast-boundaries",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|field| field.type_oid)
            .collect::<Vec<_>>(),
        vec![17, 17, 17, 21, 23, 20]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some(r"\x8000".to_owned()),
            Some(r"\x7fffffff".to_owned()),
            Some(r"\x8000000000000000".to_owned()),
            Some("-32768".to_owned()),
            Some("2147483647".to_owned()),
            Some("-9223372036854775808".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_integer_bytea_casts_in_binary_result_format() {
    // Arrange
    let statement = "integer_bytea_casts_binary_results";
    let portal = "integer_bytea_casts_binary_results_portal";
    let sql = "SELECT (-32768)::INT2::BYTEA AS small_bytes, \
                      2147483647::INT4::BYTEA AS integer_bytes, \
                      (-9223372036854775808)::INT8::BYTEA AS bigint_bytes, \
                      decode('8000', 'hex')::INT2 AS small_value, \
                      decode('7fffffff', 'hex')::INT4 AS integer_value, \
                      decode('8000000000000000', 'hex')::INT8 AS bigint_value";
    let batches = run_script(
        "issue-816-integer-bytea-binary-results",
        vec![
            Step::ParseFlush(wire::parse_frame(statement, sql)),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
                wire::describe_portal_frame(portal),
                wire::execute_frame(portal),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let result = &batches[1];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|field| (field.type_oid, field.format_code))
            .collect::<Vec<_>>(),
        vec![(17, 1), (17, 1), (17, 1), (21, 1), (23, 1), (20, 1)]
    );
    assert_eq!(
        data_row_bytes(result),
        vec![
            vec![0x80, 0x00],
            vec![0x7f, 0xff, 0xff, 0xff],
            vec![0x80, 0, 0, 0, 0, 0, 0, 0],
            vec![0x80, 0x00],
            vec![0x7f, 0xff, 0xff, 0xff],
            vec![0x80, 0, 0, 0, 0, 0, 0, 0],
        ]
    );
}
