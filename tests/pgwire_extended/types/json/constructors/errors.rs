//! Exact unsigned JSON document numbers retain their selected document meaning.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;
use cassie::types::Value;

#[path = "../inspection_null_semantics.rs"]
mod inspection_null_semantics;
#[path = "../json_collection_sql_json/nulls.rs"]
mod json_collection_sql_json_wave;
#[path = "../operators/containment.rs"]
mod jsonb_containment;
#[path = "../operators/extraction.rs"]
mod jsonb_extraction;
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
#[path = "../native_jsonb.rs"]
mod native_jsonb;
#[path = "../jsonpath/operator_overload/errors.rs"]
mod operator_overload_residuals;
#[path = "../operator_truth_tables.rs"]
mod operator_truth_tables;

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

include!("nulls.rs");

include!("semantics.rs");

include!("../numeric_carriers/errors.rs");

include!("../numeric_carriers/restart.rs");

include!("../numeric_carriers/semantics.rs");

include!("../operators/errors.rs");

include!("../operators/metadata.rs");

include!("../operators/nulls.rs");

include!("../operators/semantics.rs");

include!("../operators/concat.rs");

include!("../sql_json/nulls.rs");

include!("../sql_json/semantics.rs");

include!("../table_functions/errors.rs");

include!("../table_functions/nulls.rs");

include!("../table_functions/semantics.rs");

#[path = "../native_comparisons.rs"]
mod native_comparisons;

#[path = "../inspection_native.rs"]
mod inspection_native;

#[path = "../constructor_native.rs"]
mod constructor_native;

#[path = "../mutation_native.rs"]
mod mutation_native;

#[path = "../path_function_native.rs"]
mod path_function_native;
