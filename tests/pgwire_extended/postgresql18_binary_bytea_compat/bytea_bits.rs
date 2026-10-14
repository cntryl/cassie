use super::*;

#[test]
fn should_read_bytea_bits_from_least_significant_positions() {
    // Arrange
    let sql = "SELECT get_bit(decode('8001', 'hex'), 7) AS first_byte, \
                      get_bit(decode('8001', 'hex'), 8) AS next_byte";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-get-bit", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23; 2]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("1".to_owned()), Some("1".to_owned())]]
    );
}

#[test]
fn should_set_bytea_bits_at_least_significant_positions() {
    // Arrange
    let sql = "SELECT set_bit(decode('0000', 'hex'), 0, 1) AS first, \
                      set_bit(decode('0000', 'hex'), 15, 1) AS last";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-set-bit", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![17; 2]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r"\x0100".to_owned()), Some(r"\x0080".to_owned())]]
    );
}

#[test]
fn should_read_bytea_bytes_as_unsigned_values() {
    // Arrange
    let sql = "SELECT get_byte(decode('0102ff', 'hex'), 2) AS value";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-get-byte", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(row_description(result)[0].type_oid, 23);
    assert_eq!(wire::data_rows(result), vec![vec![Some("255".to_owned())]]);
}

#[test]
fn should_replace_bytea_bytes_with_unsigned_values() {
    // Arrange
    let sql = "SELECT set_byte(decode('0102ff', 'hex'), 1, 255) AS payload";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-set-byte", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(row_description(result)[0].type_oid, 17);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r"\x01ffff".to_owned())]]
    );
}

#[test]
fn should_count_set_bits_in_bytea_values() {
    // Arrange
    let sql = "SELECT bit_count(decode('00ff0180', 'hex')) AS set_bits";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-bit-count", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(row_description(result)[0].type_oid, 23);
    assert_eq!(wire::data_rows(result), vec![vec![Some("10".to_owned())]]);
}

#[test]
fn should_reject_bytea_access_outside_declared_bounds() {
    // Arrange
    let cases = [
        "SELECT get_bit(decode('80', 'hex'), -1)",
        "SELECT get_bit(decode('80', 'hex'), 8)",
        "SELECT set_bit(decode('80', 'hex'), 8, 1)",
        "SELECT get_byte(decode('ff', 'hex'), -1)",
        "SELECT set_byte(decode('ff', 'hex'), 1, 0)",
    ];
    let mut steps = cases
        .iter()
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect::<Vec<_>>();
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT 42 AS recovered",
    )]));

    // Act
    let batches = run_script("postgresql18-bytea-index-bounds", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() + 1);
    for (index, sql) in cases.iter().enumerate() {
        let result = &batches[index];
        assert_eq!(
            wire::error_code(result).as_deref(),
            Some("2202E"),
            "PostgreSQL 18.6 SQLSTATE for {sql}"
        );
        assert!(
            !result
                .iter()
                .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C')),
            "{sql} must not return partial result frames"
        );
        assert_eq!(
            result.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..]),
            "{sql} should restore ReadyForQuery idle state"
        );
    }
    assert_eq!(wire::error_code(&batches[cases.len()]), None);
    assert_eq!(
        wire::data_rows(&batches[cases.len()]),
        vec![vec![Some("42".to_owned())]]
    );
}
