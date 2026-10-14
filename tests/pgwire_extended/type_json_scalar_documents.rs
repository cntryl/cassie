//! Exact unsigned JSON document numbers retain their selected document meaning.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;
use cassie::types::Value;

#[path = "type_json_scalar_documents/inspection_null_semantics.rs"]
mod inspection_null_semantics;
#[path = "type_json_scalar_documents/json_collection_sql_json_wave.rs"]
mod json_collection_sql_json_wave;
#[path = "type_json_scalar_documents/json_query_quote_policies.rs"]
mod json_query_quote_policies;
#[path = "type_json_scalar_documents/json_query_returning_alias.rs"]
mod json_query_returning_alias;
#[path = "type_json_scalar_documents/jsonb_string_scalar_key_boundary.rs"]
mod jsonb_string_scalar_key_boundary;
#[path = "type_json_scalar_documents/jsonpath_accessor_grammar.rs"]
mod jsonpath_accessor_grammar;
#[path = "type_json_scalar_documents/jsonpath_item_methods.rs"]
mod jsonpath_item_methods;
#[path = "type_json_scalar_documents/jsonpath_null_semantics.rs"]
mod jsonpath_null_semantics;
#[path = "type_json_scalar_documents/jsonpath_operator_semantics.rs"]
mod jsonpath_operator_semantics;
#[path = "type_json_scalar_documents/operator_overload_residuals.rs"]
mod operator_overload_residuals;
#[path = "type_json_scalar_documents/operator_truth_tables.rs"]
mod operator_truth_tables;
#[path = "type_json_scalar_documents/record_table_functions.rs"]
mod record_table_functions;
#[path = "type_json_scalar_documents/sql_json_serialize_wave7.rs"]
mod sql_json_serialize_wave7;

const MAX_DOCUMENT: &[u8] = b"18446744073709551615";
const NEIGHBOR_DOCUMENT: &[u8] = b"18446744073709551614";
const WIDE_INTEGER_TEXT: &[u8] = b"9007199254740993";
const WIDE_INTEGER_BINARY: &[u8] = &[0, 32, 0, 0, 0, 0, 0, 1];

#[test]
fn should_reject_non_numeric_aggregate_carriers_with_exact_wire_errors() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_aggregate_errors (doc JSON, label TEXT, flag BOOLEAN)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_aggregate_errors (doc, label, flag) VALUES ($1, 'first', true), ($2, 'second', false), ($3, 'third', NULL)",
            vec![
                Value::Json(serde_json::json!(2)),
                Value::Json(serde_json::json!("not a number")),
                Value::Json(serde_json::json!(3)),
            ],
        ),
    ];
    let cases = [
        ("SUM(doc)", "function sum(json) does not exist"),
        ("AVG(doc)", "function avg(json) does not exist"),
        ("SUM(label)", "function sum(text) does not exist"),
        ("AVG(label)", "function avg(text) does not exist"),
        ("SUM(flag)", "function sum(boolean) does not exist"),
        ("AVG(flag)", "function avg(boolean) does not exist"),
        (
            "SUM(CAST(doc AS TEXT))",
            "function sum(text) does not exist",
        ),
        (
            "AVG(CAST(doc AS TEXT))",
            "function avg(text) does not exist",
        ),
        (
            "SUM(CAST(doc AS JSON))",
            "function sum(text) does not exist",
        ),
        (
            "AVG(CAST(doc AS JSON))",
            "function avg(text) does not exist",
        ),
    ];
    let mut cycles = Vec::new();
    for (expression, _) in cases {
        cycles.push(fixture::execute_cycle(
            &format!("SELECT {expression} FROM json_aggregate_errors"),
            None,
            0,
            false,
        ));
        cycles.push(fixture::execute_cycle("SELECT 1", None, 0, false));
    }

    // Act
    let batches = fixture::run_cycles("type-json-aggregate-errors", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((expression, expected), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        let frames = &pair[0];
        assert_eq!(
            wire::error_code(frames).as_deref(),
            Some("42883"),
            "{expression}"
        );
        let errors = frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1, "{expression}");
        let message = wire::parse_error_fields(&errors[0].1)
            .into_iter()
            .find_map(|(key, value)| (key == 'M').then_some(value))
            .expect("aggregate error message");
        assert_eq!(message, expected, "{expression}");
        assert_eq!(
            frames
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
                .count(),
            0,
            "failed Execute must not publish a result for {expression}"
        );
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        fixture::assert_success(&pair[1]);
        assert_eq!(wire::data_rows(&pair[1]), vec![vec![Some("1".to_string())]]);
    }
}

#[test]
fn should_preserve_declared_json_recordset_fields() {
    // Arrange
    let sql = "SELECT id, n FROM jsonb_to_recordset('[{\"id\":1,\"n\":1},{\"id\":2,\"n\":null},{\"id\":3}]'::jsonb) AS records(id INTEGER, n INTEGER) ORDER BY id";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("type-json-recordset-null-fields", &[], cycles);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("recordset RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), None],
            vec![Some("3".to_owned()), None],
        ]
    );
}

#[test]
fn should_convert_json_objects_to_declared_record_fields() {
    // Arrange
    let sql = "SELECT id, name FROM json_to_record('{\"id\":7,\"name\":\"Ada\"}') AS record(id INTEGER, name TEXT)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-to-record", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("json_to_record RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("7".to_owned()), Some("Ada".to_owned())]]
    );
}

#[test]
fn should_abort_json_recordset_on_a_later_typed_conversion_error_without_partial_rows() {
    // Arrange
    let sql = "SELECT id, n FROM jsonb_to_recordset('[{\"id\":1,\"n\":1},{\"id\":2,\"n\":\"bad\"}]'::jsonb) AS records(id INTEGER, n INTEGER)";
    let cycles = vec![
        fixture::execute_cycle(sql, None, 0, true),
        fixture::execute_cycle("SELECT 42", None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("type-json-recordset-later-conversion-error", &[], cycles);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_construct_sql_json_values_with_null_policy() {
    // Arrange
    let sql = "SELECT JSON_OBJECT('a' VALUE 1, 'b' VALUE NULL ABSENT ON NULL), JSON_ARRAY(1, NULL, 2 ABSENT ON NULL), JSON_SCALAR('hello')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-sql-json-constructors", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("SQL/JSON constructor RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![114, 114, 114]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"a" : 1}"#.to_owned()),
            Some("[1, 2]".to_owned()),
            Some(r#""hello""#.to_owned()),
        ]]
    );
}

#[test]
fn should_evaluate_jsonpath_array_accessors_in_both_modes() {
    // Arrange
    let sql = "SELECT jsonb_path_query_array('{\"a\":[1,2]}'::JSONB, 'strict $.a[*]'), jsonb_path_query_array('{\"a\":1}'::JSONB, 'lax $.a[*]')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-jsonpath-strict-lax-array-access", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath result RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3802, 3802]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[1, 2]".to_owned()), Some("[1]".to_owned())]]
    );
}

#[test]
fn should_apply_jsonpath_filter_variables_to_array_members() {
    // Arrange
    let sql = "SELECT jsonb_path_exists('{\"a\":[1,2,3]}'::JSONB, '$.a[*] ? (@ > $min)', '{\"min\":2}'::JSONB)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-filter-variable", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_type_jsonpath_literals_with_postgresql_oid() {
    // Arrange
    let sql = "SELECT '$.items[*].double()'::JSONPATH";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-literal-type", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (4072, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("$.\"items\"[*].double()".to_owned())]]
    );
}

#[test]
fn should_evaluate_jsonpath_match_predicates() {
    // Arrange
    let sql = "SELECT jsonb_path_match('{\"a\":2}'::JSONB, '$.a == 2')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-match-predicate", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (16, 1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"t")]
    );
}

#[test]
fn should_return_typed_sql_json_query_exists_results() {
    // Arrange
    let sql = "SELECT JSON_VALUE('{\"n\":42}', '$.n' RETURNING INTEGER), JSON_QUERY('{\"n\":[1,2]}', '$.n'), JSON_EXISTS('{\"n\":42}', '$.n')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-sql-json-query-functions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("SQL/JSON query function RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 114, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("42".to_owned()),
            Some("[1, 2]".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_bigint_returning_identity_for_json_value() {
    // Arrange
    let sql = "SELECT JSON_VALUE(\
                   '{\"n\":9223372036854775807}', \
                   '$.n' RETURNING BIGINT\
               ) AS value";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-value-returning-bigint", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (20, 8, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("9223372036854775807".to_owned())]]
    );
}

#[test]
fn should_preserve_jsonb_returning_identity_for_json_query() {
    // Arrange
    let sql = "SELECT JSON_QUERY(\
                   '{\"n\":[1,2]}'::JSONB, \
                   '$.n' RETURNING JSONB\
               ) AS value";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-query-returning-jsonb", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3_802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[1, 2]".to_owned())]]
    );
}

#[test]
fn should_propagate_sql_null_through_typed_json_query_functions() {
    // Arrange
    let sql =
        "SELECT JSON_VALUE(NULL::JSONB, '$.n' RETURNING BIGINT ERROR ON EMPTY ERROR ON ERROR), \
                      JSON_QUERY(NULL::JSONB, '$.n' RETURNING JSONB ERROR ON EMPTY ERROR ON ERROR)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-null-typed-results", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("typed SQL/JSON RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(20, 8, -1), (3_802, -1, -1)]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![None, None]]);
}

#[test]
fn should_report_json_value_returning_conversion_error_with_sqlstate() {
    // Arrange
    let invalid_value = fixture::execute_cycle(
        "SELECT JSON_VALUE('{\"n\":\"bad\"}'::JSONB, '$.n' RETURNING INTEGER ERROR ON ERROR)",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-json-value-returning-error",
        &[],
        vec![invalid_value, recovery],
    );

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22P02"));
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
    assert!(
        frames
            .iter()
            .all(|(tag, _)| !matches!(*tag, b'T' | b'D' | b'C')),
        "a conversion error must not publish result rows"
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_apply_sql_json_policy_distinctions_with_declared_result_types() {
    // Arrange
    let value_policy = fixture::execute_cycle(
        "SELECT JSON_VALUE('{}', '$.n' RETURNING INTEGER DEFAULT 7 ON EMPTY DEFAULT 9 ON ERROR), JSON_VALUE('{\"n\":\"bad\"}', '$.n' RETURNING INTEGER DEFAULT 7 ON EMPTY DEFAULT 9 ON ERROR)",
        None,
        0,
        true,
    );
    let query_empty = fixture::execute_cycle(
        "SELECT JSON_QUERY('{}', '$.n' RETURNING JSONB EMPTY ARRAY ON EMPTY ERROR ON ERROR)",
        None,
        0,
        true,
    );
    let query_multiple = fixture::execute_cycle(
        "SELECT JSON_QUERY('{\"n\":[1,2]}', '$.n[*]' RETURNING JSONB ERROR ON EMPTY ERROR ON ERROR)",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-sql-json-empty-error-policies",
        &[],
        vec![value_policy, query_empty, query_multiple, recovery],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_VALUE policy RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("7".to_owned()), Some("9".to_owned())]]
    );

    let frames = &batches[1];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3_802, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("[]".to_owned())]]);

    let frames = &batches[2];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22034"));
    let errors = frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    let message = wire::parse_error_fields(&errors[0].1)
        .into_iter()
        .find_map(|(key, value)| (key == 'M').then_some(value))
        .expect("JSON_QUERY cardinality error message");
    assert_eq!(
        message,
        "JSON path expression in JSON_QUERY must return single item when no wrapper is requested"
    );
    assert!(wire::data_rows(frames).is_empty());

    fixture::assert_success(&batches[3]);
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_classify_json_object_unique_keys() {
    // Arrange
    let sql = "SELECT '{\"a\":1}' IS JSON OBJECT WITH UNIQUE KEYS, '{\"a\":1,\"a\":2}' IS JSON OBJECT WITH UNIQUE KEYS";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-sql-json-unique-key-predicate", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("IS JSON RowDescription");
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
fn should_delete_a_jsonb_key_when_set_lax_receives_sql_null() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_set_lax_values (document JSONB, path TEXT[], replacement JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_set_lax_values VALUES ($1, $2, $3)",
            vec![
                Value::Json(serde_json::json!({"a": 1})),
                Value::Json(serde_json::json!(["a"])),
                Value::Null,
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_set_lax(document, path, replacement, TRUE, 'delete_key') FROM issue_810_jsonb_set_lax_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-set-lax-delete", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"{}")]
    );
}

#[test]
fn should_insert_jsonb_values_at_array_paths() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_insert_values (document JSONB, path TEXT[], new_value JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_insert_values VALUES ($1, $2, $3)",
            vec![
                Value::Json(serde_json::json!([1, 3])),
                Value::Json(serde_json::json!(["1"])),
                Value::Json(serde_json::json!(2)),
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_insert(document, path, new_value) FROM issue_810_jsonb_insert_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-insert-array-path", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"[1, 2, 3]")]
    );
}

#[test]
fn should_order_json_array_aggregate_without_null_inputs() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_array_aggregate_order_input (n INT, value TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_array_aggregate_order_input VALUES (2, 'b'), (1, 'a'), (3, NULL)",
            Vec::new(),
        ),
    ];
    let sql = "SELECT JSON_ARRAYAGG(value ORDER BY n ABSENT ON NULL) FROM json_array_aggregate_order_input";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "type-json-array-aggregate-order-null-policy",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_ARRAYAGG RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 114);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[\"a\", \"b\"]".to_owned())]]
    );
}

#[test]
fn should_enforce_unique_json_object_aggregate_keys_with_sqlstate() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_object_aggregate_unique_input (key TEXT, value TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_object_aggregate_unique_input VALUES ('x', 'a'), ('y', 'b')",
            Vec::new(),
        ),
        (
            "CREATE TABLE json_object_aggregate_duplicate_input (key TEXT, value TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_object_aggregate_duplicate_input VALUES ('x', 'a'), ('x', 'b')",
            Vec::new(),
        ),
    ];
    let success = fixture::execute_cycle(
        "SELECT JSON_OBJECTAGG(key VALUE value WITH UNIQUE KEYS) FROM json_object_aggregate_unique_input",
        None,
        0,
        true,
    );
    let duplicate = fixture::execute_cycle(
        "SELECT JSON_OBJECTAGG(key VALUE value WITH UNIQUE KEYS) FROM json_object_aggregate_duplicate_input",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "type-json-object-aggregate-unique-keys",
        &setup,
        vec![success, duplicate, recovery],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{ \"x\" : \"a\", \"y\" : \"b\" }".to_owned())]]
    );
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22030"));
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_preserve_ordinary_json_numeric_aggregates_in_both_wire_formats() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_numeric_aggregates (doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_numeric_aggregates (doc) VALUES ($1), ($2), ($3)",
            vec![
                Value::Json(serde_json::json!(2)),
                Value::Json(serde_json::json!(3)),
                Value::Null,
            ],
        ),
    ];
    let cases: [(&str, &[u8], &[u8]); 2] = [
        ("SUM(doc)", b"5", &[64, 20, 0, 0, 0, 0, 0, 0]),
        ("AVG(doc)", b"2.5", &[64, 4, 0, 0, 0, 0, 0, 0]),
    ];
    let cycles = cases
        .into_iter()
        .flat_map(|(expression, _, _)| {
            [0, 1].map(|format| {
                fixture::execute_cycle(
                    &format!("SELECT {expression} FROM json_numeric_aggregates"),
                    None,
                    format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-numeric-aggregates", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), cases.len() * 2);
    for ((_, text, binary), group) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        for (format, frames) in [0, 1].into_iter().zip(group) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (701, 8, -1), format, true);
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(if format == 0 {
                    text
                } else {
                    binary
                })]
            );
        }
    }
}

#[test]
fn should_preserve_unsigned_json_scalar_documents_across_restart() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE unsigned_json_documents (k INT, doc JSON)",
            Vec::new(),
        ),
        (
            "CREATE TABLE unsigned_json_bigints (k INT, wide BIGINT)",
            Vec::new(),
        ),
    ];
    let mut first = Vec::new();
    for input_format in [0, 1] {
        for result_format in [0, 1] {
            let key = input_format * 2 + result_format + 1;
            first.push(fixture::execute_cycle(
                &format!(
                    "INSERT INTO unsigned_json_documents (k, doc) VALUES ({key}, $1) RETURNING doc"
                ),
                Some((114, input_format, Some(MAX_DOCUMENT))),
                result_format,
                true,
            ));
            first.push(fixture::execute_cycle(
                &format!(
                    "INSERT INTO unsigned_json_bigints (k, wide) VALUES ({key}, $1) RETURNING wide"
                ),
                Some((
                    20,
                    input_format,
                    Some(if input_format == 0 {
                        WIDE_INTEGER_TEXT
                    } else {
                        WIDE_INTEGER_BINARY
                    }),
                )),
                result_format,
                true,
            ));
        }
    }
    let reads = [0, 1]
        .into_iter()
        .flat_map(|format| {
            [
                fixture::execute_cycle(
                    "SELECT doc FROM unsigned_json_documents WHERE k = 1",
                    None,
                    format,
                    true,
                ),
                fixture::execute_cycle(
                    "SELECT wide FROM unsigned_json_bigints WHERE k = 1",
                    None,
                    format,
                    true,
                ),
            ]
        })
        .collect::<Vec<_>>();
    first.extend(reads.clone());

    // Act
    let stages = fixture::run_cycles_across_restart(
        "type-json-scalar-u64-restart",
        &setup,
        vec![first, reads],
    );

    // Assert
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].len(), 12);
    assert_eq!(stages[1].len(), 4);
    for (index, frames) in stages[0].iter().enumerate() {
        let format = if index < 8 {
            (index / 2) % 2
        } else {
            (index - 8) / 2
        };
        assert_number_roundtrip(
            frames,
            index % 2 == 0,
            i16::try_from(format).expect("format"),
        );
    }
    for (index, frames) in stages[1].iter().enumerate() {
        assert_number_roundtrip(
            frames,
            index % 2 == 0,
            i16::try_from(index / 2).expect("format"),
        );
    }
}

#[test]
fn should_preserve_unsigned_json_numeric_predicates() {
    // Arrange
    let setup = unsigned_json_predicate_setup();
    let inputs = [(MAX_DOCUMENT, vec![1, 2]), (NEIGHBOR_DOCUMENT, vec![3])];
    let cycles = inputs
        .iter()
        .flat_map(|(body, _)| {
            [0, 1].into_iter().flat_map(move |input_format| {
                [0, 1].map(move |format| {
                    fixture::execute_cycle(
                        "SELECT k FROM unsigned_json_predicates WHERE doc = $1 ORDER BY k",
                        Some((114, input_format, Some(body))),
                        format,
                        true,
                    )
                })
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-scalar-u64-predicate", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 8);
    for ((_, expected), group) in inputs.iter().zip(batches.as_chunks::<4>().0) {
        for (index, frames) in group.iter().enumerate() {
            let format = i16::try_from(index % 2).expect("format");
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (23, 4, -1), format, true);
            let rows = expected
                .iter()
                .map(|key: &i32| {
                    if format == 0 {
                        fixture::one_field_row(key.to_string().as_bytes())
                    } else {
                        fixture::one_field_row(&key.to_be_bytes())
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(fixture::data_row_payloads(frames), rows);
        }
    }
}

#[test]
fn should_group_unsigned_json_scalar_documents_without_rounding() {
    // Arrange
    let setup = unsigned_json_predicate_setup();
    let cycles = [0, 1]
        .map(|format| {
            fixture::execute_cycle(
                "SELECT doc, COUNT(*) AS n FROM unsigned_json_predicates GROUP BY doc ORDER BY doc",
                None,
                format,
                true,
            )
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles("type-json-scalar-u64-groups", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 2);
    for (format, frames) in [0, 1].into_iter().zip(batches) {
        fixture::assert_success(&frames);
        let descriptions = frames
            .iter()
            .filter(|(tag, _)| *tag == b'T')
            .map(|(_, body)| wire::parse_row_description(body))
            .collect::<Vec<_>>();
        assert_eq!(descriptions.len(), 2);
        for (index, columns) in descriptions.into_iter().enumerate() {
            assert_eq!(columns.len(), 2);
            assert_eq!(
                (
                    columns[0].type_oid,
                    columns[0].type_size,
                    columns[0].type_mod
                ),
                (114, -1, -1)
            );
            assert_eq!(
                (
                    columns[1].type_oid,
                    columns[1].type_size,
                    columns[1].type_mod
                ),
                (20, 8, -1)
            );
            for column in columns {
                assert_eq!(column.format_code, if index == 0 { 0 } else { format });
            }
        }
        let expected = [(NEIGHBOR_DOCUMENT, 1_i64), (MAX_DOCUMENT, 2_i64)].map(|(doc, count)| {
            let mut row = fixture::one_field_row(doc);
            row[1] = 2;
            let count_field = if format == 0 {
                fixture::one_field_row(count.to_string().as_bytes())
            } else {
                fixture::one_field_row(&count.to_be_bytes())
            };
            row.extend_from_slice(&count_field[2..]);
            row
        });
        assert_eq!(fixture::data_row_payloads(&frames), expected.to_vec());
    }
}

#[test]
fn should_preserve_signed_json_numeric_key_normalization() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE signed_json_number_controls (doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO signed_json_number_controls (doc) VALUES ($1), ($2)",
            vec![
                Value::Json(serde_json::from_str("1").expect("integer JSON document")),
                Value::Json(serde_json::from_str("1.0").expect("float JSON document")),
            ],
        ),
    ];
    let queries = [
        "SELECT COUNT(*) AS n FROM signed_json_number_controls GROUP BY doc",
        "SELECT COUNT(*) AS n FROM signed_json_number_controls WHERE CAST(doc AS FLOAT) = 1",
    ];
    let cycles = queries
        .into_iter()
        .flat_map(|sql| [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true)))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-signed-key-sibling", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for (index, frames) in batches.iter().enumerate() {
        let format = i16::try_from(index % 2).expect("format");
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (20, 8, -1), format, true);
        let expected = if format == 0 {
            fixture::one_field_row(b"2")
        } else {
            fixture::one_field_row(&2_i64.to_be_bytes())
        };
        assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
    }
}

fn assert_number_roundtrip(frames: &fixture::Frames, json: bool, format: i16) {
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(
        frames,
        if json { (114, -1, -1) } else { (20, 8, -1) },
        format,
        true,
    );
    let bytes = if json {
        MAX_DOCUMENT
    } else if format == 0 {
        WIDE_INTEGER_TEXT
    } else {
        WIDE_INTEGER_BINARY
    };
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(bytes)]
    );
}

fn unsigned_json_predicate_setup() -> Vec<(&'static str, Vec<Value>)> {
    vec![
        (
            "CREATE TABLE unsigned_json_predicates (k INT, doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO unsigned_json_predicates (k, doc) VALUES (1, $1), (2, $1), (3, $2)",
            vec![
                Value::Json(serde_json::from_slice(MAX_DOCUMENT).expect("exact JSON u64 maximum")),
                Value::Json(
                    serde_json::from_slice(NEIGHBOR_DOCUMENT).expect("exact adjacent JSON u64"),
                ),
            ],
        ),
    ]
}

#[test]
fn should_preserve_unsigned_json_documents_in_conflict_excluded_rows() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE unsigned_json_conflicts (k INT PRIMARY KEY, doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO unsigned_json_conflicts (k, doc) VALUES (1, '0')",
            Vec::new(),
        ),
    ];
    let cases = [MAX_DOCUMENT, NEIGHBOR_DOCUMENT];
    let cycles = cases.into_iter().flat_map(|body| {
        [0, 1].into_iter().flat_map(move |input_format| {
            [0, 1].map(move |format| {
                fixture::execute_cycle(
                    "INSERT INTO unsigned_json_conflicts (k, doc) VALUES (1, $1) ON CONFLICT (k) DO UPDATE SET doc = excluded.doc RETURNING doc",
                    Some((114, input_format, Some(body))),
                    format,
                    true,
                )
            })
        })
    }).collect();

    // Act
    let batches = fixture::run_cycles("type-json-u64-excluded-row", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 8);
    for (body, group) in cases.into_iter().zip(batches.as_chunks::<4>().0) {
        for (index, frames) in group.iter().enumerate() {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(
                frames,
                (114, -1, -1),
                i16::try_from(index % 2).expect("format"),
                true,
            );
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(body)]
            );
        }
    }
}

#[test]
fn should_preserve_ordinary_json_scalar_predicate_carriers() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE ordinary_json_number_controls (k INT, doc JSON)",
            Vec::new(),
        ),
        (
            "INSERT INTO ordinary_json_number_controls (k, doc) VALUES (1, $1), (2, $2), (3, $3)",
            vec![
                Value::Json(
                    serde_json::from_str("9007199254740993").expect("exact signed JSON number"),
                ),
                Value::Json(serde_json::from_str("3.5").expect("floating JSON document")),
                Value::Json(serde_json::from_str("false").expect("Boolean JSON document")),
            ],
        ),
    ];
    let cases = [
        (
            "SELECT doc FROM ordinary_json_number_controls WHERE CAST(doc AS BIGINT) = 9007199254740993",
            b"9007199254740993".as_slice(),
        ),
        (
            "SELECT doc FROM ordinary_json_number_controls WHERE CAST(doc AS FLOAT) = 3.5",
            b"3.5".as_slice(),
        ),
        (
            "SELECT doc FROM ordinary_json_number_controls WHERE CAST(doc AS BOOLEAN) = FALSE",
            b"false".as_slice(),
        ),
    ];
    let cycles = cases
        .into_iter()
        .flat_map(|(sql, _)| [0, 1].map(|format| fixture::execute_cycle(sql, None, format, true)))
        .collect();

    // Act
    let batches = fixture::run_cycles("type-json-ordinary-scalar-predicates", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 6);
    for ((_, body), pair) in cases.into_iter().zip(batches.as_chunks::<2>().0) {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (114, -1, -1), format, true);
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(body)]
            );
        }
    }
}

#[test]
fn should_preserve_json_null_when_expanding_jsonb_array_elements_text() {
    // Arrange
    let sql = "SELECT value FROM jsonb_array_elements_text('[null,\"a\",\"c\"]'::jsonb) AS value ORDER BY value NULLS FIRST";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-array-elements-text", &[], cycles);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON array-element RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![None],
            vec![Some("a".to_owned())],
            vec![Some("c".to_owned())],
        ]
    );
}

#[test]
fn should_preserve_jsonb_values_when_expanding_array_elements() {
    // Arrange
    let sql = "SELECT jsonb_array_elements('[1,null,\"x\"]'::jsonb)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-array-elements", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB array-element RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3_802]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned())],
            vec![Some("null".to_owned())],
            vec![Some("\"x\"".to_owned())],
        ]
    );
}

#[test]
fn should_expand_jsonb_object_fields_in_select_list() {
    // Arrange
    let sql = "SELECT jsonb_each_text('{\"b\":2,\"a\":null}'::jsonb)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-each-text", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (2_249, -1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![
            fixture::one_field_row(b"(a,)"),
            fixture::one_field_row(b"(b,2)"),
        ]
    );
}

#[test]
fn should_expand_jsonb_values_as_select_list_records() {
    // Arrange
    let sql = "SELECT jsonb_each('{\"b\":2,\"a\":null}'::jsonb)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-each", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (2_249, -1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![
            fixture::one_field_row(b"(a,null)"),
            fixture::one_field_row(b"(b,2)"),
        ]
    );
}

#[test]
fn should_expand_json_table_rows() {
    // Arrange
    let sql = r#"SELECT id FROM JSON_TABLE('[{"id":7},{"id":9}]', '$[*]' COLUMNS (id INTEGER PATH '$.id')) AS j ORDER BY id"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-table-rows", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"7"), fixture::one_field_row(b"9")]
    );
}

#[test]
fn should_expand_nested_json_table_paths_with_ordinality() {
    // Arrange
    let sql = r#"SELECT * FROM JSON_TABLE('[{"id":7,"items":[{"n":1},{"n":2}]},{"id":9,"items":[]}]', '$[*]' COLUMNS (id INTEGER PATH '$.id', NESTED PATH '$.items[*]' COLUMNS (ordinal FOR ORDINALITY, n INTEGER PATH '$.n' DEFAULT 0 ON EMPTY))) AS j ORDER BY id, ordinal"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-table-nested-ordinality", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("nested JSON_TABLE RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 23]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("7".to_owned()),
                Some("1".to_owned()),
                Some("1".to_owned()),
            ],
            vec![
                Some("7".to_owned()),
                Some("2".to_owned()),
                Some("2".to_owned()),
            ],
            vec![Some("9".to_owned()), None, None],
        ]
    );
}

#[test]
fn should_distinguish_json_null_from_missing_or_sql_null_with_jsonb_operators() {
    // Arrange
    let sql = "SELECT ('{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB -> 'present'), ('{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB -> 'missing'), '{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB ->> 'present', '{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB #> '{nested,x}', '{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB #>> '{nested,x}', '{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB ? 'present', '{\"present\":null,\"n\":2,\"nested\":{\"x\":true}}'::JSONB ? 'missing'";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-jsonb-operator-null-missing", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB operator RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3802, 3802, 25, 3802, 25, 16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("null".to_owned()),
            None,
            None,
            Some("true".to_owned()),
            Some("true".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_jsonb_containment_with_boolean_result_identity() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE jsonb_containment_input (document JSONB, subset JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO jsonb_containment_input VALUES ('{\"a\":1,\"b\":[1,2]}', '{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document @> subset AS contains FROM jsonb_containment_input",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "type-jsonb-containment-result-identity",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (16, 1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_preserve_large_integer_values_in_jsonb_constructors() {
    // Arrange
    let sql = "SELECT jsonb_build_object('wide', 9007199254740993, 'sql_null', NULL), jsonb_build_array(1, NULL, 'x'), jsonb_typeof(jsonb_build_object('a', 1))";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-jsonb-builders-exact-values", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB builder RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3802, 3802, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"wide": 9007199254740993, "sql_null": null}"#.to_owned()),
            Some(r#"[1, null, "x"]"#.to_owned()),
            Some("object".to_owned()),
        ]]
    );
}

#[test]
fn should_order_jsonb_aggregate_with_both_json_sql_null_inputs() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE jsonb_aggregate_order_input (ordinal INT, payload JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO jsonb_aggregate_order_input VALUES (2, '{\"n\":2}'), (1, 'null'), (3, NULL), (4, '{\"n\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_agg(payload ORDER BY ordinal) FROM jsonb_aggregate_order_input",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "type-jsonb-aggregate-order-null-inputs",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)[0].type_oid,
        3802
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some(
            "[null, {\"n\": 2}, null, {\"n\": 1}]".to_owned()
        )]]
    );
}

#[test]
fn should_replace_a_jsonb_object_field_without_changing_document_type() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_set('{\"a\":1}'::JSONB, $1, jsonb_build_array(2))",
        Some((1009, 0, Some(&b"{a}"[..]))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-set-object-field", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": [2]}".to_owned())]]
    );
}

#[test]
fn should_skip_null_values_in_strict_jsonb_object_aggregation() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_jsonb_object_strict_input (key TEXT, payload JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_jsonb_object_strict_input VALUES ('a', NULL), ('b', 'null'), ('c', '1')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_object_agg_strict(key, payload) FROM issue_811_jsonb_object_strict_input",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-811-jsonb-object-aggregate-strict-null-policy",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"c\": 1}".to_owned())]]
    );
}

#[test]
fn should_skip_sql_nulls_in_the_json_strict_aggregate() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_json_aggregate_strict_values (value INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_json_aggregate_strict_values VALUES (1), (NULL), (2)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT json_agg_strict(value) FROM issue_811_json_aggregate_strict_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-811-json-aggregate-strict", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (114, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[1, 2]".to_owned())]]
    );
}

#[test]
fn should_omit_sql_null_values_in_unique_jsonb_object_aggregation() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_unique_object_values (key TEXT, value INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_unique_object_values VALUES ('a', 1), ('b', NULL)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_object_agg_unique_strict(key, value) FROM issue_811_unique_object_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-811-jsonb-object-aggregate-unique-strict",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": 1}".to_owned())]]
    );
}

#[test]
fn should_preserve_json_object_types_for_strict_unique_forms() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_json_object_variants (key TEXT, value INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_json_object_variants VALUES ('a', 1), ('b', NULL)",
            Vec::new(),
        ),
    ];
    let cases = [
        (
            "json_object_agg_strict",
            "SELECT json_object_agg_strict(key, value) FROM issue_811_json_object_variants",
            114,
            "{ \"a\" : 1 }",
        ),
        (
            "json_object_agg_unique",
            "SELECT json_object_agg_unique(key, value) FROM issue_811_json_object_variants",
            114,
            "{ \"a\" : 1, \"b\" : null }",
        ),
        (
            "jsonb_object_agg_unique",
            "SELECT jsonb_object_agg_unique(key, value) FROM issue_811_json_object_variants",
            3_802,
            "{\"a\": 1, \"b\": null}",
        ),
        (
            "json_object_agg_unique_strict",
            "SELECT json_object_agg_unique_strict(key, value) FROM issue_811_json_object_variants",
            114,
            "{ \"a\" : 1 }",
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(_, sql, _, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-811-json-object-strict-unique", &setup, cycles);

    // Assert
    let mut mismatches = Vec::new();
    for ((name, _, expected_oid, expected_value), frames) in cases.iter().zip(&batches) {
        if let Some(code) = wire::error_code(frames) {
            mismatches.push(format!("{name} returned SQLSTATE {code}"));
            continue;
        }
        let Some(description) = frames.iter().find(|(tag, _)| *tag == b'T') else {
            mismatches.push(format!("{name} returned no RowDescription"));
            continue;
        };
        let actual_oid = wire::parse_row_description(&description.1)[0].type_oid;
        if actual_oid != *expected_oid {
            mismatches.push(format!("{name} returned type OID {actual_oid}"));
        }
        let expected_rows = vec![vec![Some((*expected_value).to_owned())]];
        let actual_rows = wire::data_rows(frames);
        if actual_rows != expected_rows {
            mismatches.push(format!("{name} returned rows {actual_rows:?}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "aggregate mismatches: {mismatches:?}"
    );
}

#[test]
fn should_match_any_jsonb_object_key_from_a_text_array() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_any_keys (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_any_keys VALUES ('{\"a\":1,\"b\":2}')",
            Vec::new(),
        ),
    ];
    let sql =
        "SELECT document ?| '{b,missing}', document ?| '{missing}' FROM issue_809_jsonb_any_keys";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-any-key-array", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::parse_row_description(
            &frames
                .iter()
                .find(|(tag, _)| *tag == b'T')
                .expect("JSONB any-key RowDescription")
                .1
        )
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
fn should_require_every_jsonb_object_key_from_a_text_array() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_all_keys (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_all_keys VALUES ('{\"a\":1,\"b\":2}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT document ?& '{a,b}', document ?& '{a,missing}' FROM issue_809_jsonb_all_keys";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-all-key-array", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("t".to_owned()), Some("f".to_owned())]]
    );
}

#[test]
fn should_merge_jsonb_objects_with_right_hand_key_precedence() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_merge_values (left_document JSONB, right_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_merge_values VALUES ('{\"a\":1,\"b\":0}', '{\"b\":2,\"c\":3}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT (left_document || right_document)::TEXT FROM issue_809_jsonb_merge_values";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-concatenate-objects", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": 1, \"b\": 2, \"c\": 3}".to_owned())]]
    );
}

#[test]
fn should_delete_a_jsonb_object_key_with_the_minus_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_delete_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_delete_values VALUES ('{\"a\":1,\"b\":2}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT (document - 'a')::TEXT FROM issue_809_jsonb_delete_values";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-delete-object-key", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"b\": 2}".to_owned())]]
    );
}

#[test]
fn should_count_elements_in_a_jsonb_array() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_length_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_length_values VALUES ('[2,4]')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_array_length(document) FROM issue_810_jsonb_length_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-array-length", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("2".to_owned())]]);
}

#[test]
fn should_expand_jsonb_object_keys_as_text_rows() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_keys_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_keys_values VALUES ('{\"b\":2,\"a\":1}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT jsonb_object_keys(document) FROM issue_810_jsonb_keys_values ORDER BY 1";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-object-keys", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("a".to_owned())], vec![Some("b".to_owned())]]
    );
}

#[test]
fn should_detect_any_jsonb_key_from_a_text_array_function() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_exists_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_exists_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT jsonb_exists_any(document, '{a,b}') FROM issue_810_jsonb_exists_values";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-jsonb-exists-array-functions",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_require_all_jsonb_keys_from_a_text_array_function() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_exists_all_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_exists_all_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_exists_all(document, '{a,b}') FROM issue_810_jsonb_exists_all_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-810-jsonb-exists-all-array-function",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("f".to_owned())]]);
}

#[test]
fn should_strip_jsonb_null_fields() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_strip_nulls_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_strip_nulls_values VALUES ('{\"a\":null,\"items\":[null,1]}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT jsonb_strip_nulls(document)::TEXT FROM issue_810_jsonb_strip_nulls_values";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-strip-null-arrays", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"items\": [null, 1]}".to_owned())]]
    );
}

#[test]
fn should_strip_null_elements_from_jsonb_arrays_when_requested() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_strip_array_nulls_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_strip_array_nulls_values VALUES ('{\"a\":null,\"items\":[null,1]}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_strip_nulls(document, TRUE)::TEXT FROM issue_810_jsonb_strip_array_nulls_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-810-jsonb-strip-array-null-elements",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"items\": [1]}".to_owned())]]
    );
}

#[test]
fn should_convert_scalar_values_with_to_jsonb() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_to_jsonb_values (value INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_to_jsonb_values VALUES (42)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT to_jsonb(value) FROM issue_810_to_jsonb_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-to-jsonb-scalar", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("42".to_owned())]]);
}

#[test]
fn should_return_jsonb_text_with_the_arrow_text_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_arrow_text_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_arrow_text_values VALUES ('{\"label\":\"value\"}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document ->> 'label' FROM issue_809_arrow_text_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-arrow-text", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("value".to_owned())]]
    );
}

#[test]
fn should_extract_nested_jsonb_values_with_the_path_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_values VALUES ('{\"nested\":{\"x\":7}}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document #> '{nested,x}' FROM issue_809_path_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_owned())]]);
}

#[test]
fn should_extract_nested_jsonb_scalars_as_text_with_the_path_text_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_text_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_text_values VALUES ('{\"nested\":{\"x\":7}}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document #>> '{nested,x}' FROM issue_809_path_text_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path-text", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_owned())]]);
}

#[test]
fn should_detect_jsonb_object_key_presence_without_other_operators() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_key_presence_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_key_presence_values VALUES ('{\"present\":null}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document ? 'present', document ? 'missing' FROM issue_809_key_presence_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-key-presence", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("t".to_owned()), Some("f".to_owned())]]
    );
}

#[test]
fn should_return_the_jsonb_type_name_for_a_stored_document() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_typeof_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_typeof_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_typeof(document) FROM issue_810_typeof_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-typeof", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("object".to_owned())]]
    );
}

#[test]
fn should_construct_jsonb_arrays_from_scalar_plus_sql_null_inputs() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_build_array_values (value INTEGER)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_build_array_values VALUES (42)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_build_array(value, NULL)::TEXT FROM issue_810_build_array_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-build-array", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[42, null]".to_owned())]]
    );
}

#[test]
fn should_pretty_print_jsonb_objects_with_nested_arrays() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_pretty_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_pretty_values VALUES ('{\"a\":1,\"b\":[2,3]}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_pretty(document) FROM issue_810_pretty_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-pretty", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some(
            "{\n    \"a\": 1,\n    \"b\": [\n        2,\n        3\n    ]\n}".to_owned()
        )]]
    );
}

#[test]
fn should_apply_the_jsonb_path_exists_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_exists_operator_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_exists_operator_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document @? '$.a' FROM issue_809_path_exists_operator_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path-exists-operator", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_apply_the_jsonb_path_match_operator() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_path_match_operator_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_path_match_operator_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document @@ '$.a == 1' FROM issue_809_path_match_operator_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-path-match-operator", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_subscript_a_jsonb_object_by_key() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_subscript_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_subscript_values VALUES ('{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document['a'] FROM issue_809_subscript_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-key-subscript", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("1".to_owned())]]);
}

#[test]
fn should_extract_jsonb_path_text_with_the_path_function() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_extract_path_text_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_extract_path_text_values VALUES ('{\"nested\":{\"x\":7}}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_extract_path_text(document, 'nested', 'x') FROM issue_810_extract_path_text_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-extract-path-text", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("7".to_owned())]]);
}

#[test]
fn should_replace_a_jsonb_value_at_a_path_with_jsonb_set() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_set_values (document JSONB, path TEXT[], replacement JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_set_values VALUES ($1, $2, $3)",
            vec![
                Value::Json(serde_json::json!({"a": 1, "b": 1})),
                Value::Json(serde_json::json!(["a"])),
                Value::Json(serde_json::json!(2)),
            ],
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_set(document, path, replacement)::TEXT FROM issue_810_jsonb_set_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-set-path", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": 2, \"b\": 1}".to_owned())]]
    );
}

#[test]
fn should_apply_jsonpath_predicate_operators_with_boolean_wire_types() {
    // Arrange
    let sql = "SELECT payload @? '$.items[*] ? (@.qty >= 2)'::JSONPATH, payload @@ '$.active == true'::JSONPATH FROM (VALUES ('{\"active\":true,\"items\":[{\"qty\":1},{\"qty\":2}]}'::JSONB)) AS input(payload)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonpath-predicate-operators", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath predicate operator RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("t".to_owned()), Some("t".to_owned())]]
    );
}

#[test]
fn should_distinguish_lax_singleton_wrap_from_strict_jsonpath_error() {
    // Arrange
    let lax = fixture::execute_cycle(
        "SELECT jsonb_path_query_array('{\"a\":1}'::JSONB, 'lax $.a[*]')",
        None,
        0,
        true,
    );
    let strict = fixture::execute_cycle(
        "SELECT jsonb_path_query_array('{\"a\":1}'::JSONB, 'strict $.a[*]')",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonpath-strict-lax-singleton",
        &[],
        vec![lax, strict, recovery],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (3_802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("[1]".to_owned())]]
    );

    let frames = &batches[1];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22039"));
    let errors = frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    let message = wire::parse_error_fields(&errors[0].1)
        .into_iter()
        .find_map(|(key, value)| (key == 'M').then_some(value))
        .expect("strict JSONPath error message");
    assert_eq!(
        message,
        "jsonpath wildcard array accessor can only be applied to an array"
    );
    assert!(wire::data_rows(frames).is_empty());

    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_suppress_jsonpath_structural_errors_only_for_silent_forms() {
    // Arrange
    let unsuppressed = fixture::execute_cycle(
        "SELECT jsonb_path_exists('{\"a\":1}'::JSONB, 'strict $.a[*]')",
        None,
        0,
        true,
    );
    let suppressed = fixture::execute_cycle(
        "SELECT jsonb_path_exists('{\"a\":1}'::JSONB, 'strict $.a[*]', '{}'::JSONB, true), '{\"a\":1}'::JSONB @? 'strict $.a[*]'",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonpath-silent-structural-errors",
        &[],
        vec![unsuppressed, suppressed, recovery],
    );

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22039"));
    let errors = frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    let message = wire::parse_error_fields(&errors[0].1)
        .into_iter()
        .find_map(|(key, value)| (key == 'M').then_some(value))
        .expect("unsuppressed JSONPath structural error message");
    assert_eq!(
        message,
        "jsonpath wildcard array accessor can only be applied to an array"
    );
    assert!(wire::data_rows(frames).is_empty());

    let frames = &batches[1];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("silent JSONPath forms RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("f".to_owned()), Some("f".to_owned())]]
    );

    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_type_json_table_missing_or_json_null_fields_as_sql_null() {
    // Arrange
    let sql = r#"SELECT id, required, optional, ord FROM JSON_TABLE('[{"id":7,"required":"one","optional":null},{"id":9,"required":"two"}]', '$[*]' COLUMNS (id BIGINT PATH '$.id', required TEXT PATH '$.required', optional TEXT PATH '$.optional', ord FOR ORDINALITY)) AS j ORDER BY ord"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-table-column-types", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("typed JSON_TABLE RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 25, 25, 20]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("7".to_owned()),
                Some("one".to_owned()),
                None,
                Some("1".to_owned()),
            ],
            vec![
                Some("9".to_owned()),
                Some("two".to_owned()),
                None,
                Some("2".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_order_distinct_jsonb_aggregate_inputs_with_sql_null() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_jsonb_distinct_order (ordinal INT, payload JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_jsonb_distinct_order VALUES (2, '{\"n\":2}'), (4, NULL), (3, '{\"n\":2}'), (1, '{\"n\":1}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_agg(DISTINCT payload ORDER BY payload) FROM issue_811_jsonb_distinct_order",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-811-jsonb-aggregate-distinct-order",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3_802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[{\"n\": 1}, {\"n\": 2}, null]".to_owned())]]
    );
}
