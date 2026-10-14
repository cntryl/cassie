//! PostgreSQL JSON set-returning and record conversion behavior.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use cassie::types::Value;

#[test]
fn should_convert_jsonb_objects_to_declared_single_record_fields() {
    // Arrange
    let sql = "SELECT item.n FROM jsonb_to_record('{\"n\":42}'::JSONB) AS item(n BIGINT)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-to-record", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (20, 8, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("42".to_owned())]]);
}

#[test]
fn should_expand_json_recordset_objects_with_declared_text_fields() {
    // Arrange
    let sql = "SELECT item.label FROM json_to_recordset('[{\"label\":\"Ada\"},{\"label\":\"Bo\"}]'::JSON) AS item(label TEXT) ORDER BY item.label";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-recordset", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("Ada".to_owned())], vec![Some("Bo".to_owned())]]
    );
}

#[test]
fn should_expand_parameterized_jsonb_recordset_with_declared_int8_fields() {
    // Arrange
    let document = br#"[{"n":42},{"n":43}]"#;
    let sql = "SELECT item.n FROM jsonb_to_recordset($1::JSONB) AS item(n BIGINT)";
    let cycle = fixture::execute_cycle(sql, Some((3_802, 0, Some(document))), 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-recordset-parameter", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (20, 8, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("42".to_owned())], vec![Some("43".to_owned())]]
    );
}

#[test]
fn should_expand_jsonb_array_values_laterally_with_ordinality() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue829_json_lateral_input (id BIGINT, payload JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue829_json_lateral_input VALUES ($1, $2)",
            vec![Value::Int64(1), Value::Json(serde_json::json!([10, 20]))],
        ),
        (
            "INSERT INTO issue829_json_lateral_input VALUES ($1, $2)",
            vec![Value::Int64(2), Value::Json(serde_json::json!([]))],
        ),
    ];
    let sql = "SELECT source.id, expanded.value, expanded.ordinality FROM issue829_json_lateral_input AS source LEFT JOIN LATERAL jsonb_array_elements(source.payload) WITH ORDINALITY AS expanded(value, ordinality) ON TRUE ORDER BY source.id, expanded.ordinality";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-lateral-jsonb-ordinality", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("lateral JSON array RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 3_802, 20]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some("10".to_owned()),
                Some("1".to_owned())
            ],
            vec![
                Some("1".to_owned()),
                Some("20".to_owned()),
                Some("2".to_owned())
            ],
            vec![Some("2".to_owned()), None, None],
        ]
    );
}

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
fn should_distinguish_jsonb_populate_record_missing_fields_from_json_null() {
    // Arrange
    let sql = "SELECT jsonb_populate_record(ROW(CAST(5 AS BIGINT), CAST('base' AS TEXT)), CAST('{}' AS JSONB)), jsonb_populate_record(ROW(CAST(5 AS BIGINT), CAST('base' AS TEXT)), CAST('{\"f1\":null}' AS JSONB)), jsonb_populate_record(ROW(CAST(5 AS BIGINT), CAST('base' AS TEXT)), CAST('{\"f2\":\"x\"}' AS JSONB))";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-populate-record-null-missing", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("jsonb_populate_record RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![2_249, 2_249, 2_249]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("(5,base)".to_owned()),
            Some("(,base)".to_owned()),
            Some("(5,x)".to_owned()),
        ]]
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
fn should_distinguish_jsonb_populate_recordset_missing_fields_from_json_null() {
    // Arrange
    let sql = "SELECT jsonb_populate_recordset(ROW(CAST(5 AS BIGINT), CAST('base' AS TEXT)), '[{},{\"f1\":null},{\"f2\":\"x\"}]'::JSONB)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-829-populate-recordset-null-missing",
        &[],
        vec![cycle],
    );

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
            vec![Some("(5,base)".to_owned())],
            vec![Some("(,base)".to_owned())],
            vec![Some("(5,x)".to_owned())],
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

#[test]
fn should_populate_declared_recordset_fields_with_postgresql_types() {
    // Arrange
    let sql = "SELECT fields.id, fields.label, fields.payload FROM jsonb_populate_recordset(NULL::record, '[{\"id\":9223372036854775807,\"label\":null,\"payload\":{\"k\":1}},{\"label\":\"two\"}]'::jsonb) AS fields(id BIGINT, label TEXT, payload JSONB) ORDER BY fields.id NULLS LAST";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-populate-recordset-types", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("jsonb_populate_recordset RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 25, 3_802]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("9223372036854775807".to_owned()),
                None,
                Some("{\"k\": 1}".to_owned()),
            ],
            vec![None, Some("two".to_owned()), None],
        ]
    );
}

#[test]
fn should_abort_recordset_on_a_later_typed_conversion_error_without_partial_rows() {
    // Arrange
    let malformed = fixture::execute_cycle(
        "SELECT fields.value FROM jsonb_populate_recordset(NULL::record, '[{\"value\":1},{\"value\":\"invalid\"}]'::jsonb) AS fields(value INTEGER) ORDER BY fields.value",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-829-jsonb-populate-recordset-later-conversion-error",
        &[],
        vec![malformed, recovery],
    );

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22P02"));
    let errors = frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    let message = wire::parse_error_fields(&errors[0].1)
        .into_iter()
        .find_map(|(key, value)| (key == 'M').then_some(value))
        .expect("record conversion error message");
    assert_eq!(
        message,
        "invalid input syntax for type integer: \"invalid\""
    );
    assert!(wire::data_rows(frames).is_empty());

    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}
