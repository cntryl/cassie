//! PostgreSQL JSON set-returning and record conversion behavior.

use super::{fixture, wire};

#[test]
fn should_report_jsonb_populate_record_conversion_validity() {
    // Arrange
    let sql = "SELECT jsonb_populate_record_valid(ROW(CAST(NULL AS BIGINT), CAST(NULL AS TEXT)), '{\"f1\":\"11\",\"f2\":\"Ada\"}'::JSONB), jsonb_populate_record_valid(ROW(CAST(NULL AS BIGINT), CAST(NULL AS TEXT)), '{\"f1\":\"bad\",\"f2\":\"Ada\"}'::JSONB)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-populate-record-validity", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("record validity RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("t".to_owned()), Some("f".to_owned())]]
    );
}

#[test]
fn should_expand_jsonb_array_elements_as_text() {
    // Arrange
    let sql = "SELECT jsonb_array_elements_text('[1,null,\"x\"]'::JSONB)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-array-elements-text", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("jsonb_array_elements_text RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned())],
            vec![None],
            vec![Some("x".to_owned())],
        ]
    );
}

#[test]
fn should_populate_anonymous_records_from_jsonb() {
    // Arrange
    let sql = "SELECT jsonb_populate_record(ROW(CAST(NULL AS BIGINT), CAST(NULL AS TEXT)), '{\"f1\":\"11\",\"f2\":\"Ada\"}'::JSONB)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-populate-record-jsonb", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("jsonb_populate_record RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        2_249
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("(11,Ada)".to_owned())]]
    );
}

#[test]
fn should_populate_anonymous_records_from_json() {
    // Arrange
    let sql = "SELECT json_populate_record(ROW(CAST(NULL AS BIGINT), CAST(NULL AS TEXT)), '{\"f1\":\"11\",\"f2\":\"Ada\"}'::JSON)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-populate-record-json", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("json_populate_record RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        2_249
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("(11,Ada)".to_owned())]]
    );
}

#[test]
fn should_expand_jsonb_populate_recordset_as_record_values() {
    // Arrange
    let sql = "SELECT jsonb_populate_recordset(ROW(CAST(NULL AS BIGINT), CAST(NULL AS TEXT)), '[{\"f1\":\"1\",\"f2\":\"A\"},{\"f1\":\"2\",\"f2\":\"B\"}]'::JSONB)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-populate-recordset-jsonb", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("jsonb_populate_recordset RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        2_249
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("(1,A)".to_owned())],
            vec![Some("(2,B)".to_owned())],
        ]
    );
}

#[test]
fn should_expand_json_populate_recordset_as_record_values() {
    // Arrange
    let sql = "SELECT json_populate_recordset(ROW(CAST(NULL AS BIGINT), CAST(NULL AS TEXT)), '[{\"f1\":\"1\",\"f2\":\"A\"},{\"f1\":\"2\",\"f2\":\"B\"}]'::JSON)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-populate-recordset-json", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("json_populate_recordset RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        2_249
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("(1,A)".to_owned())],
            vec![Some("(2,B)".to_owned())],
        ]
    );
}
