use crate::app::{Cassie, CassieError};
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::{
    column_vector_test_support::{
        read_column_vector_batches, read_row_documents, TestColumnVectorBatch,
    },
    query_scan_control_test_guard,
};
use crate::runtime::QueryExecutionControls;
use cassie_core::types::lanes::{LaneValidity, RawLanes};
use cassie_core::types::Value;
use std::time::Instant;

const ALL_TYPES: &str = "s SMALLINT, i INT, b BIGINT, f FLOAT, t BOOLEAN, x TEXT, v VARCHAR(8), \
    c CHAR(4), d DATE, ts TIMESTAMP, u UUID, j JSON, jb JSONB, n NUMERIC(10,2)";
const ALL_TYPE_FIELDS: [&str; 14] = [
    "s", "i", "b", "f", "t", "x", "v", "c", "d", "ts", "u", "j", "jb", "n",
];

fn fixture() -> Cassie {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::from_env().expect("runtime config"))
            .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE typed ({ALL_TYPES}) WITH (storage = column_store)"),
            vec![],
        )
        .expect("create typed table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO typed VALUES \
             (1, 70000, 9007199254740993, 1.5, true, 'alpha', 'beta', 'ab', '2024-01-02', \
              '2024-01-02 03:04:05', '00000000-0000-0000-0000-000000000001', '{\"a\":[1,2]}', \
              '{\"b\":null}', 12.34), \
             (NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL), \
             (-2, -70000, -9007199254740993, -0.25, false, '', 'é', 'cd', '1999-12-31', \
              '1999-12-31 23:59:59', '00000000-0000-0000-0000-000000000002', 'null', '[]', 0.00)",
            vec![],
        )
        .expect("insert typed rows");
    cassie
}

fn is_valid(validity: &LaneValidity, lane: usize) -> bool {
    match validity {
        LaneValidity::AllValid => true,
        LaneValidity::Flags(flags) => flags[lane],
        LaneValidity::Words(words) => words[lane / 64] >> (lane % 64) & 1 == 1,
    }
}

/// The values a lane carrier holds, with NULL for every invalid lane.
fn lane_values(lanes: &RawLanes) -> Vec<Value> {
    match lanes {
        RawLanes::I64 { values, validity } => values
            .iter()
            .enumerate()
            .map(|(lane, value)| {
                if is_valid(validity, lane) {
                    Value::Int64(*value)
                } else {
                    Value::Null
                }
            })
            .collect(),
        RawLanes::F64 { values, validity } => values
            .iter()
            .enumerate()
            .map(|(lane, value)| {
                if is_valid(validity, lane) {
                    Value::Float64(*value)
                } else {
                    Value::Null
                }
            })
            .collect(),
        RawLanes::Bool { values, validity } => values
            .iter()
            .enumerate()
            .map(|(lane, value)| {
                if is_valid(validity, lane) {
                    Value::Bool(*value != 0)
                } else {
                    Value::Null
                }
            })
            .collect(),
        RawLanes::Utf8 {
            bytes,
            offsets,
            validity,
        } => (0..offsets.len() - 1)
            .map(|lane| {
                if is_valid(validity, lane) {
                    Value::String(bytes[offsets[lane]..offsets[lane + 1]].to_owned())
                } else {
                    Value::Null
                }
            })
            .collect(),
        RawLanes::Values(values) => values.clone(),
    }
}

fn lanes_by_row(batches: &[TestColumnVectorBatch], fields: usize) -> Vec<(String, Vec<Value>)> {
    let mut rows = Vec::new();
    for batch in batches {
        let columns = batch.lanes.iter().map(lane_values).collect::<Vec<_>>();
        assert_eq!(columns.len(), fields);
        for (row, id) in batch.ids.iter().enumerate() {
            rows.push((
                id.clone(),
                columns.iter().map(|column| column[row].clone()).collect(),
            ));
        }
    }
    rows
}

fn document_values(cassie: &Cassie) -> Result<Vec<(String, Vec<Value>)>, CassieError> {
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    let (schema, documents) =
        read_row_documents(&cassie.midge, "typed", &controls.storage_request())?;
    documents
        .into_iter()
        .map(|(id, document)| {
            let values = ALL_TYPE_FIELDS
                .iter()
                .map(|name| {
                    let data_type = &schema
                        .fields
                        .iter()
                        .find(|field| field.name == *name)
                        .expect("schema field")
                        .data_type;
                    document.get(*name).map_or(Ok(Value::Null), |json| {
                        crate::executor::scan::try_json_to_typed_value(json, data_type)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok((id, values))
        })
        .collect()
}

#[test]
fn should_decode_every_type_into_the_value_the_document_path_builds() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let cassie = fixture();
    let controls = QueryExecutionControls::from_limits(&cassie.runtime.limits(), Instant::now());
    let requested = ALL_TYPE_FIELDS.map(Some);

    // Act
    let batches = read_column_vector_batches(
        &cassie.midge,
        "typed",
        &requested,
        true,
        &controls.storage_request(),
    )
    .expect("vector read")
    .expect("column table reader");
    let mut vector = lanes_by_row(&batches, ALL_TYPE_FIELDS.len());
    let mut documents = document_values(&cassie).expect("document values");
    vector.sort_by(|left, right| left.0.cmp(&right.0));
    documents.sort_by(|left, right| left.0.cmp(&right.0));

    // Assert
    assert_eq!(vector.len(), 3);
    assert_eq!(vector, documents);
}
