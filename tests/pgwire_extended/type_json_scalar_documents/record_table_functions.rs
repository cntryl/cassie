//! PostgreSQL JSON record conversion and JSON_TABLE contracts over pgwire.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[path = "expansion_resource_limits.rs"]
mod expansion_resource_limits;
#[path = "record_table_functions/json_table_encoding_path_aliases.rs"]
mod json_table_encoding_path_aliases;
#[path = "record_table_functions/json_table_later_row_errors.rs"]
mod json_table_later_row_errors;
#[path = "record_table_functions/json_table_nested_semantics.rs"]
mod json_table_nested_semantics;
#[path = "record_table_functions/jsonb_populate_recordset.rs"]
mod jsonb_populate_recordset;
#[path = "record_table_functions/recordset_json_output_types.rs"]
mod recordset_json_output_types;
#[path = "record_table_functions/recordset_output_typmods.rs"]
mod recordset_output_typmods;
#[path = "record_table_functions/recordset_parameter_inference.rs"]
mod recordset_parameter_inference;

fn column_type_oids(frames: &fixture::Frames) -> Vec<i32> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(&description.1)
        .iter()
        .map(|column| column.type_oid)
        .collect()
}

#[test]
fn should_convert_parameterized_json_documents_to_declared_record_types() {
    // Arrange
    let document = br#"{"id":9223372036854775807,"name":"Ada","payload":{"k":1}}"#;
    let json = fixture::execute_cycle(
        "SELECT item.id, item.name, item.payload FROM json_to_record($1::JSON) AS item(id BIGINT, name TEXT, payload JSONB)",
        Some((114, 0, Some(document))),
        0,
        true,
    );
    let jsonb = fixture::execute_cycle(
        "SELECT item.id, item.name, item.payload FROM jsonb_to_record($1::JSONB) AS item(id BIGINT, name TEXT, payload JSONB)",
        Some((3_802, 0, Some(document))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("json-record-parameter-types", &[], vec![json, jsonb]);

    // Assert
    for frames in batches {
        fixture::assert_success(&frames);
        assert_eq!(column_type_oids(&frames), vec![20, 25, 3_802]);
        assert_eq!(
            wire::data_rows(&frames),
            vec![vec![
                Some("9223372036854775807".to_owned()),
                Some("Ada".to_owned()),
                Some("{\"k\": 1}".to_owned()),
            ]]
        );
    }
}

#[test]
fn should_distinguish_json_populate_record_missing_from_json_null() {
    // Arrange
    let single = fixture::execute_cycle(
        "SELECT json_populate_record(ROW(5::BIGINT, 'base'::TEXT), '{}'::JSON), json_populate_record(ROW(5::BIGINT, 'base'::TEXT), '{\"f1\":null}'::JSON)",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("json-populate-record-base-values", &[], vec![single]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(column_type_oids(&batches[0]), vec![2_249, 2_249]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("(5,base)".to_owned()),
            Some("(,base)".to_owned())
        ]]
    );
}

#[test]
fn should_null_json_populate_recordset_fields_for_explicit_json_null() {
    // Arrange
    let set = fixture::execute_cycle(
        "SELECT json_populate_recordset(ROW(5::BIGINT, 'base'::TEXT), '[{},{\"f1\":null},{\"f2\":\"x\"}]'::JSON)",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("json-populate-recordset-null-fields", &[], vec![set]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(column_type_oids(&batches[0]), vec![2_249]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![
            vec![Some("(5,base)".to_owned())],
            vec![Some("(,base)".to_owned())],
            vec![Some("(5,x)".to_owned())],
        ]
    );
}

#[test]
fn should_report_jsonb_populate_record_conversion_validity() {
    // Arrange
    let sql = "SELECT jsonb_populate_record_valid(ROW(5::BIGINT, 'base'::TEXT), '{}'::JSONB), jsonb_populate_record_valid(ROW(5::BIGINT, 'base'::TEXT), '{\"f1\":null}'::JSONB), jsonb_populate_record_valid(ROW(5::BIGINT, 'base'::TEXT), '{\"f1\":\"bad\"}'::JSONB)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("json-populate-record-valid-null-missing", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![16, 16, 16]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_filter_json_table_rows_through_named_jsonpath_variables() {
    // Arrange
    let sql = r#"SELECT ordinal, id, price FROM JSON_TABLE('[{"id":1,"price":4},{"id":2,"price":9},{"id":3,"price":12}]'::JSONB, '$[*] ? (@.price >= $min)' AS filtered_rows PASSING 10 AS min COLUMNS (ordinal FOR ORDINALITY, id INTEGER PATH '$.id', price INTEGER PATH '$.price')) AS t"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("json-table-passing-named-path", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![20, 23, 23]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("1".to_owned()),
            Some("3".to_owned()),
            Some("12".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_json_table_format_with_quote_modes() {
    // Arrange
    let sql = r#"SELECT all_values, conditional_single, conditional_many, without_wrapper, keep_name, omit_name FROM JSON_TABLE('{"items":[{"name":"alpha"},{"name":"beta"}],"one":[{"name":"solo"}],"name":"Ada"}'::JSONB, '$' COLUMNS (all_values JSONB FORMAT JSON PATH '$.items[*].name' WITH UNCONDITIONAL WRAPPER, conditional_single JSONB FORMAT JSON PATH '$.one[*].name' WITH CONDITIONAL WRAPPER, conditional_many JSONB FORMAT JSON PATH '$.items[*].name' WITH CONDITIONAL WRAPPER, without_wrapper JSONB FORMAT JSON PATH '$.one[*].name' WITHOUT WRAPPER, keep_name TEXT PATH '$.name' KEEP QUOTES, omit_name TEXT FORMAT JSON PATH '$.name' OMIT QUOTES)) AS t"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("json-table-format-wrappers-quotes", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        column_type_oids(frames),
        vec![3_802, 3_802, 3_802, 3_802, 25, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[\"alpha\", \"beta\"]".to_owned()),
            Some("\"solo\"".to_owned()),
            Some("[\"alpha\", \"beta\"]".to_owned()),
            Some("\"solo\"".to_owned()),
            Some("\"Ada\"".to_owned()),
            Some("Ada".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_json_table_column_typmods_over_pgwire() {
    // Arrange
    let sql = r#"SELECT id, amount, label
        FROM JSON_TABLE(
            '[{"id":7,"amount":12.345,"label":"Ada"}]'::JSONB,
            '$[*]' COLUMNS (
                id INTEGER PATH '$.id',
                amount NUMERIC(8,2) PATH '$.amount',
                label VARCHAR(5) PATH '$.label'
            )
        ) AS records"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-table-column-typmods", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_TABLE RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1), (1_700, -1, 524_294), (1_043, -1, 9)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("7".to_owned()),
            Some("12.35".to_owned()),
            Some("Ada".to_owned()),
        ]]
    );
}

#[test]
fn should_report_json_table_exists_for_present_null_or_absent_path() {
    // Arrange
    let sql = r#"SELECT id, has_value, has_missing
        FROM JSON_TABLE(
            '[{"id":1,"value":null},{"id":2}]'::JSONB,
            '$[*]' COLUMNS (
                id INTEGER PATH '$.id',
                has_value BOOLEAN EXISTS PATH '$.value',
                has_missing BOOLEAN EXISTS PATH '$.missing'
            )
        ) AS expanded
        ORDER BY id"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("json-table-exists-path-null-presence", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![23, 16, 16]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some("t".to_owned()),
                Some("f".to_owned())
            ],
            vec![
                Some("2".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned())
            ],
        ]
    );
}

#[test]
fn should_apply_json_table_empty_default_conversion_error_policies() {
    // Arrange
    let sql = r#"SELECT missing_default, empty_object, bad_default, bad_null FROM JSON_TABLE('[{"n":"bad"}]'::JSONB, '$[*]' COLUMNS (missing_default INTEGER PATH '$.missing' DEFAULT 7 ON EMPTY, empty_object JSONB FORMAT JSON PATH '$.missing' EMPTY OBJECT ON EMPTY, bad_default INTEGER PATH '$.n' DEFAULT 9 ON ERROR, bad_null INTEGER PATH '$.n' NULL ON ERROR)) AS t"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("json-table-default-empty-object-errors", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![23, 3_802, 23, 23]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("7".to_owned()),
            Some("{}".to_owned()),
            Some("9".to_owned()),
            None,
        ]]
    );
}

#[test]
fn should_report_json_table_path_errors_without_partial_rows() {
    // Arrange
    let empty = fixture::execute_cycle(
        "SELECT * FROM JSON_TABLE('{\"a\":1}'::JSONB, 'strict $.missing[*]' COLUMNS (x INTEGER PATH '$.x') EMPTY ON ERROR) AS t",
        None,
        0,
        true,
    );
    let error = fixture::execute_cycle(
        "SELECT * FROM JSON_TABLE('{\"a\":1}'::JSONB, 'strict $.missing[*]' COLUMNS (x INTEGER PATH '$.x') ERROR ON ERROR) AS t",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "json-table-path-error-recovery",
        &[],
        vec![empty, error, recovery],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert!(wire::data_rows(&batches[0]).is_empty());
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("2203A"));
    assert!(wire::data_rows(&batches[1]).is_empty());
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_report_json_table_multiple_values_without_wrapper() {
    // Arrange
    let query = fixture::execute_cycle(
        "SELECT * FROM JSON_TABLE('{\"items\":[1,2]}'::JSONB, '$' COLUMNS (items JSONB FORMAT JSON PATH '$.items[*]' WITHOUT WRAPPER ERROR ON ERROR)) AS t",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles("json-table-wrapper-cardinality", &[], vec![query, recovery]);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22034"));
    assert!(wire::data_rows(&batches[0]).is_empty());
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_update_rows_from_json_table_sources() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_table_update_target (id INTEGER PRIMARY KEY, label TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_table_update_target VALUES (1, 'old'), (2, 'keep')",
            Vec::new(),
        ),
    ];
    let update = fixture::execute_cycle(
        "UPDATE json_table_update_target AS target SET label = source.label FROM JSON_TABLE('[{\"id\":1,\"label\":\"updated\"}]'::JSONB, '$[*]' COLUMNS (id INTEGER PATH '$.id', label TEXT PATH '$.label')) AS source WHERE target.id = source.id RETURNING target.id, target.label",
        None,
        0,
        true,
    );
    let readback = fixture::execute_cycle(
        "SELECT id, label FROM json_table_update_target ORDER BY id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("json-table-update-source", &setup, vec![update, readback]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(column_type_oids(&batches[0]), vec![23, 25]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("1".to_owned()), Some("updated".to_owned())]]
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![
            vec![Some("1".to_owned()), Some("updated".to_owned())],
            vec![Some("2".to_owned()), Some("keep".to_owned())],
        ]
    );
}

#[test]
fn should_delete_rows_using_json_table_sources() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_table_delete_target (id INTEGER PRIMARY KEY, label TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_table_delete_target VALUES (1, 'keep'), (2, 'remove')",
            Vec::new(),
        ),
    ];
    let delete = fixture::execute_cycle(
        "DELETE FROM json_table_delete_target AS target USING JSON_TABLE('[{\"id\":2}]'::JSONB, '$[*]' COLUMNS (id INTEGER PATH '$.id')) AS source WHERE target.id = source.id RETURNING target.id, target.label",
        None,
        0,
        true,
    );
    let readback = fixture::execute_cycle(
        "SELECT id, label FROM json_table_delete_target ORDER BY id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("json-table-delete-source", &setup, vec![delete, readback]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(column_type_oids(&batches[0]), vec![23, 25]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("2".to_owned()), Some("remove".to_owned())]]
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("1".to_owned()), Some("keep".to_owned())]]
    );
}

#[test]
fn should_merge_rows_from_json_table_sources() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_table_merge_target (id INTEGER PRIMARY KEY, label TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_table_merge_target VALUES (1, 'old'), (2, 'remove')",
            Vec::new(),
        ),
    ];
    let merge = fixture::execute_cycle(
        "MERGE INTO json_table_merge_target AS target USING JSON_TABLE('[{\"id\":1,\"label\":\"merged\"},{\"id\":3,\"label\":\"inserted\"}]'::JSONB, '$[*]' COLUMNS (id INTEGER PATH '$.id', label TEXT PATH '$.label')) AS source ON target.id = source.id WHEN MATCHED THEN UPDATE SET label = source.label WHEN NOT MATCHED THEN INSERT (id, label) VALUES (source.id, source.label)",
        None,
        0,
        true,
    );
    let readback = fixture::execute_cycle(
        "SELECT id, label FROM json_table_merge_target ORDER BY id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("json-table-merge-source", &setup, vec![merge, readback]);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(column_type_oids(&batches[1]), vec![23, 25]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![
            vec![Some("1".to_owned()), Some("merged".to_owned())],
            vec![Some("3".to_owned()), Some("inserted".to_owned())],
        ]
    );
}
