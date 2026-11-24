use super::*;

#[test]
fn should_return_bit_string_operator_values_with_postgresql_oids() {
    // Arrange
    let sql = "SELECT B'1001' || B'011' AS concatenated, \
                      B'101101' & B'110011' AS bit_and, \
                      B'101101' | B'110011' AS bit_or, \
                      B'101101' # B'110011' AS bit_xor, \
                      ~B'10001' AS bit_not, \
                      B'10001' << 3 AS shifted_left, \
                      B'10001' >> 2 AS shifted_right";

    // Act
    let batches = run_script(
        "issue-816-bit-string-operator-wire-results",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|field| (field.type_oid, field.format_code))
            .collect::<Vec<_>>(),
        vec![
            (1562, 0),
            (1560, 0),
            (1560, 0),
            (1560, 0),
            (1560, 0),
            (1560, 0),
            (1560, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("1001011".to_owned()),
            Some("100001".to_owned()),
            Some("111111".to_owned()),
            Some("011110".to_owned()),
            Some("01110".to_owned()),
            Some("01000".to_owned()),
            Some("00100".to_owned()),
        ]]
    );
}

#[test]
fn should_return_bit_string_function_values_with_postgresql_oids() {
    // Arrange
    let sql = "SELECT bit_count(B'10110110') AS set_bits, \
                      bit_length(B'10110110') AS bit_length, \
                      length(B'10110110') AS bit_length_alias, \
                      octet_length(B'101101101') AS bytes, \
                      position(B'011' IN B'110110110') AS match_position, \
                      substring(B'110110101' FROM 3 FOR 4) AS extracted, \
                      overlay(B'110110101' PLACING B'0011' FROM 4 FOR 3) AS overlaid, \
                      get_bit(B'10110110', 0) AS first_bit, \
                      get_bit(B'10110110', 7) AS last_bit, \
                      set_bit(B'10110110', 2, 0) AS changed";

    // Act
    let batches = run_script(
        "issue-816-bit-string-function-wire-results",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|field| (field.type_oid, field.format_code))
            .collect::<Vec<_>>(),
        vec![
            (20, 0),
            (23, 0),
            (23, 0),
            (23, 0),
            (23, 0),
            (1560, 0),
            (1560, 0),
            (23, 0),
            (23, 0),
            (1560, 0),
        ]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("5".to_owned()),
            Some("8".to_owned()),
            Some("8".to_owned()),
            Some("2".to_owned()),
            Some("3".to_owned()),
            Some("0110".to_owned()),
            Some("1100011101".to_owned()),
            Some("1".to_owned()),
            Some("0".to_owned()),
            Some("10010110".to_owned()),
        ]]
    );
}
