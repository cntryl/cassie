//! Exact unsigned JSON document numbers retain their selected document meaning.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;
use cassie::types::Value;

#[path = "../inspection_null_semantics.rs"]
mod inspection_null_semantics;
#[path = "../json_collection_sql_json/nulls.rs"]
mod json_collection_sql_json_wave;
#[path = "../json_query_quote_policies.rs"]
mod json_query_quote_policies;
#[path = "../json_query_returning_alias.rs"]
mod json_query_returning_alias;
#[path = "../jsonb_string_scalar_key_boundary.rs"]
mod jsonb_string_scalar_key_boundary;
#[path = "../jsonpath/jsonpath_accessor_grammar.rs"]
mod jsonpath_accessor_grammar;
#[path = "../jsonpath/jsonpath_item_methods.rs"]
mod jsonpath_item_methods;
#[path = "../jsonpath/jsonpath_null_semantics.rs"]
mod jsonpath_null_semantics;
#[path = "../jsonpath/jsonpath_operator_semantics.rs"]
mod jsonpath_operator_semantics;
#[path = "../jsonpath/operator_overload/errors.rs"]
mod operator_overload_residuals;
#[path = "../operator_truth_tables.rs"]
mod operator_truth_tables;
#[path = "../record_table_functions.rs"]
mod record_table_functions;
#[path = "../sql_json_serialize/semantics.rs"]
mod sql_json_serialize_wave7;

const MAX_DOCUMENT: &[u8] = b"18446744073709551615";
const NEIGHBOR_DOCUMENT: &[u8] = b"18446744073709551614";
const WIDE_INTEGER_TEXT: &[u8] = b"9007199254740993";
const WIDE_INTEGER_BINARY: &[u8] = &[0, 32, 0, 0, 0, 0, 0, 1];

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

include!("nulls.rs");

include!("semantics.rs");

include!("../numeric_carriers/errors.rs");

include!("../numeric_carriers/restart.rs");

include!("../numeric_carriers/semantics.rs");

include!("../operators/errors.rs");

include!("../operators/metadata.rs");

include!("../operators/nulls.rs");

include!("../operators/semantics.rs");

include!("../sql_json/nulls.rs");

include!("../sql_json/semantics.rs");

include!("../table_functions/errors.rs");

include!("../table_functions/nulls.rs");

include!("../table_functions/semantics.rs");
