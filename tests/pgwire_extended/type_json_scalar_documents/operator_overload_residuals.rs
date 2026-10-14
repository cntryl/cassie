//! PostgreSQL 18.6 JSONB operator overload and JSONPath expression residuals for #809.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_extract_json_documents_through_arrow_path_overloads() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_json_operator_values (document JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_json_operator_values VALUES ('{\"nested\":{\"x\":7,\"label\":\"Ada\"},\"items\":[4,5]}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document -> 'nested', document ->> 'nested', \
         document #> '{nested,x}', document #>> '{nested,x}', \
         document -> 'items' -> 1, document -> 'items' ->> -1, \
         document #> '{items,1}', document #>> '{items,-1}' \
         FROM issue_809_json_operator_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-json-arrow-path-overloads", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON extraction RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![114, 25, 114, 25, 114, 25, 114, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"x":7,"label":"Ada"}"#.to_owned()),
            Some(r#"{"x":7,"label":"Ada"}"#.to_owned()),
            Some("7".to_owned()),
            Some("7".to_owned()),
            Some("5".to_owned()),
            Some("5".to_owned()),
            Some("5".to_owned()),
            Some("5".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_jsonb_array_index_null_boundaries() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_array_index_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_array_index_values VALUES ('[\"zero\",null,\"two\"]')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document -> 1, document ->> -1, document -> 5, document ->> 5 \
         FROM issue_809_jsonb_array_index_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-array-index-null", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB array index RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3802, 25, 3802, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("null".to_owned()),
            Some("two".to_owned()),
            None,
            None,
        ]]
    );
}

#[test]
fn should_preserve_jsonb_object_equality_ordering() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_object_order (case_id INTEGER, left_document JSONB, right_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_object_order VALUES \
             (1, '{\"a\":1,\"b\":2}', '{\"b\":2,\"a\":1}'), \
             (2, '{\"a\":1}', '{\"b\":1}'), \
             (3, '{\"a\":1}', '{\"a\":2}'), (4, '[]', '{}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT case_id, left_document = right_document, left_document < right_document \
         FROM issue_809_jsonb_object_order ORDER BY case_id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-object-order", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB object comparison RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 16, 16]
    );
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
                Some("t".to_owned())
            ],
            vec![
                Some("3".to_owned()),
                Some("f".to_owned()),
                Some("t".to_owned())
            ],
            vec![
                Some("4".to_owned()),
                Some("f".to_owned()),
                Some("t".to_owned())
            ],
        ]
    );
}

#[test]
fn should_apply_jsonb_deletion_operator_overloads() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_overloads (array_document JSONB, object_document JSONB, nested_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_overloads VALUES ('[0,1,2,3]', '{\"a\":1,\"b\":2,\"c\":3}', '{\"a\":{\"b\":1,\"c\":2},\"z\":3}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT (array_document - 1)::TEXT, (array_document - (-1))::TEXT, \
         (object_document - ARRAY['a','c'])::TEXT, \
         (nested_document #- ARRAY['a','b'])::TEXT \
         FROM issue_809_jsonb_delete_overloads",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-overloads", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB deletion overload RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(25, -1, -1); 4]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("[0, 2, 3]".to_owned()),
            Some("[0, 1, 2]".to_owned()),
            Some("{\"b\": 2}".to_owned()),
            Some("{\"a\": {\"c\": 2}, \"z\": 3}".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_jsonb_array_containment_without_order_sensitivity() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_array_containment (case_id INTEGER, document JSONB, subset JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_array_containment VALUES \
             (1, '[1,2,3]', '[3,1]'), \
             (2, '{\"a\":[1,2,3]}', '{\"a\":[3,1]}'), \
             (3, '{\"a\":[1,2,3]}', '{\"a\":[4]}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT case_id, document @> subset \
         FROM issue_809_jsonb_array_containment ORDER BY case_id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-array-containment", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB array containment RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned()), Some("t".to_owned())],
            vec![Some("2".to_owned()), Some("t".to_owned())],
            vec![Some("3".to_owned()), Some("f".to_owned())],
        ]
    );
}

#[test]
fn should_concatenate_jsonb_array_scalar_shapes() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_concat_shapes (case_id INTEGER, left_document JSONB, right_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_concat_shapes VALUES \
             (1, '[1,2]', '[3]'), (2, '[1,2]', '3'), \
             (3, '1', '[2,3]'), (4, '{\"a\":1}', '[2]')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT (left_document || right_document)::TEXT \
         FROM issue_809_jsonb_concat_shapes ORDER BY case_id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-concat-shapes", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("[1, 2, 3]".to_owned())],
            vec![Some("[1, 2, 3]".to_owned())],
            vec![Some("[1, 2, 3]".to_owned())],
            vec![Some("[{\"a\": 1}, 2]".to_owned())],
        ]
    );
}

#[test]
fn should_evaluate_jsonpath_range_arithmetic_predicates() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonpath_range_values (array_document JSONB, scalar_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonpath_range_values VALUES ('[10,20,30,40]', '3')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT array_document @? '$[1 to 2] ? (@ % 2 == 0 || @ + 1 == 31)'::JSONPATH, \
         scalar_document @@ '$ + 1 == 4 && !($ == 5)'::JSONPATH, \
         jsonb_path_query_array(array_document, '$[1 to 2] ? (@ + 1 == 31)'::JSONPATH)::TEXT \
         FROM issue_809_jsonpath_range_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-range-arithmetic", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath range and arithmetic RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("[30]".to_owned()),
        ]]
    );
}

#[test]
fn should_compare_jsonpath_date_time_item_methods() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonpath_temporal_methods (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonpath_temporal_methods VALUES \
             ('{\"d\":\"2024-03-10\",\"t\":\"12:34:56\", \
               \"tz\":\"12:34:56-05:00\",\"ts\":\"2024-03-10T12:34:56\", \
               \"tstz\":\"2024-03-10T12:34:56-05:00\"}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document @@ '$.d.date() == \"2024-03-10\".date() \
         && $.t.time() == \"12:34:56\".time() \
         && $.tz.time_tz() == \"12:34:56-05:00\".time_tz() \
         && $.ts.timestamp() == \"2024-03-10T12:34:56\".timestamp() \
         && $.tstz.timestamp_tz() == \"2024-03-10T12:34:56-05:00\".timestamp_tz()'::JSONPATH \
         FROM issue_809_jsonpath_temporal_methods",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-temporal-methods", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (16, 1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_report_malformed_json_text_sqlstate() {
    // Arrange
    let malformed_json = fixture::execute_cycle("SELECT '{bad}'::JSONB", None, 0, true);
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-809-malformed-json-cast",
        &[],
        vec![malformed_json, recovery],
    );

    // Assert
    assert_error_without_result_rows(&batches[0], "22P02");
    assert_recovered_select(&batches[1]);
}

#[test]
fn should_report_malformed_jsonpath_text_sqlstate() {
    // Arrange
    let malformed_jsonpath = fixture::execute_cycle("SELECT '$.a ? ('::JSONPATH", None, 0, true);
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-809-malformed-jsonpath-cast",
        &[],
        vec![malformed_jsonpath, recovery],
    );

    // Assert
    assert_error_without_result_rows(&batches[0], "42601");
    assert_recovered_select(&batches[1]);
}

#[test]
fn should_reject_boolean_operand_for_jsonb_arrow_sqlstate() {
    // Arrange
    let wrong_operand = fixture::execute_cycle("SELECT '{\"a\":1}'::JSONB -> TRUE", None, 0, true);
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonb-arrow-invalid-operand",
        &[],
        vec![wrong_operand, recovery],
    );

    // Assert
    assert_error_without_result_rows(&batches[0], "42883");
    assert_recovered_select(&batches[1]);
}

fn assert_error_without_result_rows(frames: &fixture::Frames, expected_state: &str) {
    assert_eq!(wire::error_code(frames).as_deref(), Some(expected_state));
    assert!(wire::data_rows(frames).is_empty());
    assert!(!frames.iter().any(|(tag, _)| *tag == b'T'));
}

fn assert_recovered_select(frames: &fixture::Frames) {
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("42".to_owned())]]);
}
