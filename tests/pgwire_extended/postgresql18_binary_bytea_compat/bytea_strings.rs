use super::*;

#[test]
fn should_reverse_bytea_without_decoding_text() {
    // Arrange
    let sql = "SELECT reverse(decode('00112233ff', 'hex')) AS reversed";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-reverse", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(row_description(result)[0].type_oid, 17);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r"\xff33221100".to_owned())]]
    );
}

#[test]
fn should_extract_bytea_substrings_by_byte_position() {
    // Arrange
    let sql = "SELECT substring(decode('0011223344', 'hex') FROM 2 FOR 3) AS portion";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-substring", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(row_description(result)[0].type_oid, 17);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r"\x112233".to_owned())]]
    );
}

#[test]
fn should_find_bytea_substrings_by_byte_position() {
    // Arrange
    let sql = "SELECT position(decode('1122', 'hex') IN decode('0011223344', 'hex'))";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-position", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(row_description(result)[0].type_oid, 23);
    assert_eq!(wire::data_rows(result), vec![vec![Some("2".to_owned())]]);
}

#[test]
fn should_overlay_bytea_with_replacement_bytes() {
    // Arrange
    let sql =
        "SELECT overlay(decode('0011223344', 'hex') PLACING decode('aabb', 'hex') FROM 3 FOR 1)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-overlay", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(row_description(result)[0].type_oid, 17);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r"\x0011aabb3344".to_owned())]]
    );
}

#[test]
fn should_trim_bytea_using_a_byte_set() {
    // Arrange
    let sql = "SELECT btrim(decode('0001020100', 'hex'), decode('00', 'hex'))";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-trim", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(row_description(result)[0].type_oid, 17);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r"\x010201".to_owned())]]
    );
}

#[test]
fn should_convert_bytea_between_named_encodings() {
    // Arrange
    let sql = "SELECT convert(decode('c3a9', 'hex'), 'UTF8', 'LATIN1') AS latin1";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-convert-encoding", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(row_description(result)[0].type_oid, 17);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r"\xe9".to_owned())]]
    );
}

#[test]
fn should_report_bytea_length_metrics() {
    // Arrange
    let sql = "SELECT length(decode('c3a9', 'hex')) AS bytes, \
                      length(decode('c3a9', 'hex'), 'UTF8') AS characters, \
                      bit_length(decode('c3a9', 'hex')) AS bits, \
                      octet_length(decode('c3a9', 'hex')) AS octets";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-bytea-lengths", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        row_description(result)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23; 4]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("2".to_owned()),
            Some("1".to_owned()),
            Some("16".to_owned()),
            Some("2".to_owned()),
        ]]
    );
}

#[test]
fn should_decode_bytea_text_escape_sequences() {
    // Arrange
    let sql = r"SET bytea_output = 'escape'; SELECT $q$\000\\\377$q$::bytea AS escaped_bytes";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-816-bytea-text-escapes", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    assert_eq!(row_description(result)[0].type_oid, 17);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r"\000\\\377".to_owned())]]
    );
}
