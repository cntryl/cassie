//! Vector reader tests (#1358 S2).

use super::{
    ColumnVectorBatch, ColumnVectorOutput, ColumnVectorReadSnapshot, COLUMN_VECTOR_BATCH_ROWS,
};
use crate::adapter::{
    query_scan_control_test_guard, CassieError, Midge, RowDecode, RowTraversal, StorageError,
    StorageFamily, StorageRequestContext, StorageValueHooks,
};
use cassie_core::request::{Interruption, RequestCancellation, RequestMemory};
use cassie_core::types::lanes::{LaneValidity, RawLanes};
use cassie_core::types::{DataType, FieldSchema, Schema, Value};
use std::sync::Arc;

struct Fixture {
    midge: Midge,
}

fn fixture() -> Fixture {
    let midge = Midge::new_in_memory("postgres").expect("memory Midge");
    midge.ensure_families_ready().expect("families");
    midge
        .install_value_hooks(vector_test_hooks())
        .expect("install test hooks");
    Fixture { midge }
}

fn vector_test_hooks() -> StorageValueHooks {
    StorageValueHooks::new(
        |bytes| {
            serde_json::from_slice(bytes).map_err(|error| StorageError::Parse(error.to_string()))
        },
        |value, data_type| {
            if let Some(value) =
                cassie_core::types::extended::temporal_column_value(value, data_type)
            {
                return Ok(Value::Typed(value));
            }
            Err(StorageError::Unsupported(format!(
                "test fixture does not decode {} values",
                data_type.type_name()
            )))
        },
        |_, _| {
            Err(StorageError::Unsupported(
                "graph validation is not part of this fixture".to_owned(),
            ))
        },
        |_, _, _| Err(StorageError::Unsupported("scalar evaluation".to_owned())),
        |_, _, _| Err(StorageError::Unsupported("scalar predicate".to_owned())),
    )
}

#[derive(Clone)]
struct TestRequest {
    cancellation: RequestCancellation,
    memory: RequestMemory,
    scan_control: Option<Arc<crate::adapter::QueryScanControlScope>>,
}

impl TestRequest {
    fn new(budget: usize) -> Self {
        Self {
            cancellation: RequestCancellation::new(),
            memory: RequestMemory::new(budget),
            scan_control: crate::adapter::current_query_scan_control_scope(),
        }
    }

    fn context(&self) -> StorageRequestContext<'_> {
        StorageRequestContext::new(
            None,
            Interruption::new(None, &self.cancellation),
            &self.memory,
        )
        .with_scan_control(self.scan_control.as_deref())
    }

    fn current_query_memory_bytes(&self) -> usize {
        self.memory.current_bytes()
    }

    fn peak_query_memory_bytes(&self) -> usize {
        self.memory.peak_bytes()
    }
}

fn field(name: &str, data_type: DataType) -> FieldSchema {
    FieldSchema {
        name: name.to_owned(),
        data_type,
        nullable: true,
    }
}

impl Fixture {
    fn table(&self, name: &str, fields: Vec<FieldSchema>, rows: Vec<(String, serde_json::Value)>) {
        let schema = Schema { fields };
        let metadata = super::super::super::CollectionMeta::new_with_storage_mode(
            name,
            None,
            super::super::super::CollectionStorageMode::ColumnStore,
        );
        self.midge
            .create_collection_with_meta(name, &schema, &metadata)
            .expect("create column collection");
        self.midge
            .put_documents(
                name,
                rows.into_iter()
                    .map(|(id, payload)| (Some(id), payload))
                    .collect(),
            )
            .expect("write rows");
    }

    fn controls(&self) -> TestRequest {
        TestRequest::new(64 * 1024 * 1024)
    }

    fn midge(&self) -> &Midge {
        &self.midge
    }

    fn read(
        &self,
        table: &str,
        fields: &[&str],
        traversal: RowTraversal,
        request: &TestRequest,
    ) -> Result<Vec<ColumnVectorBatch>, CassieError> {
        let requested = fields.iter().map(|field| Some(*field)).collect::<Vec<_>>();
        let mut reader = self
            .midge()
            .open_column_vector_reader(table, &requested, traversal)?
            .expect("column table reader");
        let mut batches = Vec::new();
        let controls = request.context();
        while let Some(batch) =
            reader.next_batch(self.midge(), COLUMN_VECTOR_BATCH_ROWS, &controls)?
        {
            batches.push(batch);
        }
        Ok(batches)
    }

    fn counters(&self) -> ColumnVectorReadSnapshot {
        self.midge().column_vector_reads_for_diagnostics()
    }
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

/// The row-document reading of the same table: one converted value per field.
fn document_values(fixture: &Fixture, table: &str, fields: &[&str]) -> Vec<(String, Vec<Value>)> {
    let schema = fixture.midge().row_schema(table).expect("row schema");
    let mut cursor = fixture
        .midge()
        .open_row_cursor(table, RowDecode::Full)
        .expect("open")
        .expect("cursor");
    let controls = fixture.controls();
    let mut rows = Vec::new();
    loop {
        let page = cursor
            .next_documents(fixture.midge(), 64, &controls.context())
            .expect("documents");
        if page.is_empty() {
            break;
        }
        for document in page {
            let values = fields
                .iter()
                .map(|name| {
                    let data_type = &schema
                        .fields
                        .iter()
                        .find(|field| field.name == *name)
                        .expect("field")
                        .data_type;
                    document
                        .payload
                        .get(*name)
                        .map_or(Value::Null, |json| match data_type {
                            DataType::Boolean => Value::Bool(json.as_bool().expect("boolean")),
                            DataType::SmallInt | DataType::Int | DataType::BigInt => {
                                Value::Int64(json.as_i64().expect("integer"))
                            }
                            DataType::Float => Value::Float64(json.as_f64().expect("float")),
                            DataType::Text | DataType::Varchar { .. } => {
                                Value::String(json.as_str().expect("text").to_owned())
                            }
                            _ => panic!("unexpected type in native fixture: {data_type:?}"),
                        })
                })
                .collect();
            rows.push((document.id, values));
        }
    }
    rows
}

fn typed_fixture() -> Fixture {
    let fixture = fixture();
    fixture.table(
        "typed",
        vec![
            field("s", DataType::SmallInt),
            field("f", DataType::Float),
            field("t", DataType::Boolean),
            field("x", DataType::Text),
            field("d", DataType::Date),
        ],
        vec![
            ("1".to_owned(), serde_json::json!({"s": 7000, "f": 1.5, "t": true, "x": "alpha", "d": "2024-01-02"})),
            ("2".to_owned(), serde_json::json!({"s": null, "f": null, "t": null, "x": null, "d": null})),
            ("3".to_owned(), serde_json::json!({"s": -7000, "f": -0.25, "t": false, "x": "é", "d": "1999-12-31"})),
        ],
    );
    fixture
}

#[test]
fn should_keep_scalar_types_in_native_lanes() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = typed_fixture();
    let controls = fixture.controls();

    // Act
    let batches = fixture
        .read(
            "typed",
            &["s", "f", "t", "x", "d"],
            RowTraversal::CompleteSource,
            &controls,
        )
        .expect("vector read");

    // Assert
    let lanes = &batches[0].lanes;
    assert!(matches!(lanes[0], RawLanes::I64 { .. }));
    assert!(matches!(lanes[1], RawLanes::F64 { .. }));
    assert!(matches!(lanes[2], RawLanes::Bool { .. }));
    assert!(matches!(lanes[3], RawLanes::Utf8 { .. }));
    assert!(matches!(lanes[4], RawLanes::Values(_)));
}

#[test]
fn should_decode_compressed_values_into_text_lanes() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    let long = "abc".repeat(200);
    fixture.table(
        "packed",
        vec![field("note", DataType::Text)],
        vec![
            ("1".to_owned(), serde_json::json!({"note": long})),
            ("2".to_owned(), serde_json::json!({"note": "short"})),
        ],
    );
    let schema = fixture.midge().row_schema("packed").expect("row schema");
    let prefix = Midge::column_store_field_prefix(schema.relation_id, schema.fields[0].field_id);
    let stored = fixture
        .midge()
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("stored fields");
    let compressed = stored
        .iter()
        .any(|(_, value)| value.first() == Some(&super::super::compression::COMPRESSED_TAG));
    let controls = fixture.controls();

    // Act
    let batches = fixture
        .read("packed", &["note"], RowTraversal::CompleteSource, &controls)
        .expect("vector read");

    // Assert
    assert!(compressed, "the long value is stored compressed");
    let mut values = lane_values(&batches[0].lanes[0]);
    values.sort_by_key(|value| format!("{value:?}"));
    assert!(values.contains(&Value::String(long)));
    assert!(values.contains(&Value::String("short".to_owned())));
}

#[test]
fn should_return_sql_null_for_a_stored_null_lane() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = typed_fixture();
    let controls = fixture.controls();

    // Act
    let batches = fixture
        .read("typed", &["t"], RowTraversal::CompleteSource, &controls)
        .expect("vector read");

    // Assert
    let nulls = lane_values(&batches[0].lanes[0])
        .into_iter()
        .filter(|value| *value == Value::Null)
        .count();
    assert_eq!(nulls, 1);
}

#[test]
fn should_return_null_for_a_field_added_after_the_row_was_written() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table(
        "grown",
        vec![field("a", DataType::Int), field("b", DataType::Int)],
        vec![
            ("1".to_owned(), serde_json::json!({"a": 1})),
            ("2".to_owned(), serde_json::json!({"a": 2})),
        ],
    );
    let controls = fixture.controls();

    // Act
    let batches = fixture
        .read(
            "grown",
            &["a", "b"],
            RowTraversal::CompleteSource,
            &controls,
        )
        .expect("vector read");

    // Assert
    assert_eq!(
        lane_values(&batches[0].lanes[1]),
        vec![Value::Null, Value::Null]
    );
    assert_eq!(
        lane_values(&batches[0].lanes[0]),
        vec![Value::Int64(1), Value::Int64(2)]
    );
}

#[test]
fn should_reject_a_required_field_with_no_stored_value() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table(
        "holes",
        vec![field("a", DataType::Int)],
        vec![
            ("1".to_owned(), serde_json::json!({"a": 1})),
            ("2".to_owned(), serde_json::json!({"a": 2})),
        ],
    );
    let schema = fixture.midge().row_schema("holes").expect("row schema");
    let victim = fixture
        .midge()
        .raw_scan_prefix(
            StorageFamily::Data,
            &Midge::column_store_field_prefix(schema.relation_id, schema.fields[0].field_id),
        )
        .expect("field entries")
        .into_iter()
        .next()
        .expect("one field entry")
        .0;
    fixture
        .midge()
        .raw_delete(StorageFamily::Data, &victim)
        .expect("remove one field value");
    let controls = fixture.controls();

    // Act
    let result = fixture.read("holes", &["a"], RowTraversal::CompleteSource, &controls);

    // Assert
    assert!(matches!(
        result.expect_err("missing required field"),
        StorageError::Diagnostic {
            sql_state: "XX001",
            ..
        }
    ));
}

fn orphan_key(fixture: &Fixture, table: &str, id: &str) -> Vec<u8> {
    let schema = fixture.midge().row_schema(table).expect("row schema");
    Midge::column_store_field_key(schema.relation_id, schema.fields[0].field_id, id)
}

fn plant_orphan(fixture: &Fixture, table: &str, id: &str) {
    fixture
        .midge()
        .raw_put(
            StorageFamily::Data,
            &orphan_key(fixture, table, id),
            b"\x05orphan",
        )
        .expect("orphan field key");
}

fn orphan_table(name: &str, id: &str) -> Fixture {
    let fixture = fixture();
    fixture.table(
        name,
        vec![field("a", DataType::Text)],
        vec![
            ("1".to_owned(), serde_json::json!({"a": "m"})),
            ("2".to_owned(), serde_json::json!({"a": "n"})),
        ],
    );
    plant_orphan(&fixture, name, id);
    fixture
}

#[test]
fn should_reject_an_orphan_between_markers_on_a_complete_scan() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table(
        "between_orphan",
        vec![field("a", DataType::Text)],
        vec![
            ("1".to_owned(), serde_json::json!({"a": "m"})),
            ("2".to_owned(), serde_json::json!({"a": "n"})),
        ],
    );
    let mut ids = document_values(&fixture, "between_orphan", &["a"])
        .into_iter()
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    ids.sort();
    plant_orphan(&fixture, "between_orphan", &format!("{}\u{1}", ids[0]));
    let controls = fixture.controls();

    // Act
    let result = fixture.read(
        "between_orphan",
        &["a"],
        RowTraversal::CompleteSource,
        &controls,
    );

    // Assert
    assert!(matches!(
        result.expect_err("orphan"),
        StorageError::Diagnostic {
            sql_state: "XX001",
            ..
        }
    ));
}

#[test]
fn should_reject_an_orphan_after_the_last_marker_on_a_complete_scan() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = orphan_table("tail_orphan", "~~~~");
    let controls = fixture.controls();

    // Act
    let result = fixture.read(
        "tail_orphan",
        &["a"],
        RowTraversal::CompleteSource,
        &controls,
    );

    // Assert
    assert!(matches!(
        result.expect_err("orphan"),
        StorageError::Diagnostic {
            sql_state: "XX001",
            ..
        }
    ));
}

#[test]
fn should_skip_an_orphan_on_a_visited_row_scan() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = orphan_table("visited_orphan", "0000");
    let controls = fixture.controls();

    // Act
    let batches = fixture
        .read(
            "visited_orphan",
            &["a"],
            RowTraversal::VisitedRows,
            &controls,
        )
        .expect("visited scan skips orphans");

    // Assert
    assert_eq!(
        batches.iter().map(|batch| batch.ids.len()).sum::<usize>(),
        2
    );
}

#[test]
fn should_split_rows_into_batches_of_at_most_1024() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table(
        "wide",
        vec![field("a", DataType::Int)],
        (1..=COLUMN_VECTOR_BATCH_ROWS + 1)
            .map(|value| (format!("{value:04}"), serde_json::json!({"a": value})))
            .collect(),
    );
    let controls = fixture.controls();

    // Act
    let batches = fixture
        .read("wide", &["a"], RowTraversal::CompleteSource, &controls)
        .expect("vector read");

    // Assert
    let sizes = batches
        .iter()
        .map(|batch| batch.ids.len())
        .collect::<Vec<_>>();
    assert_eq!(sizes, vec![COLUMN_VECTOR_BATCH_ROWS, 1]);
    let ids = batches
        .iter()
        .flat_map(|batch| batch.ids.iter().cloned())
        .collect::<Vec<_>>();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "batches follow id order");
}

#[test]
fn should_count_one_batch_of_lanes_with_no_row_documents() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table(
        "counted",
        vec![
            field("a", DataType::Int),
            field("b", DataType::Text),
            field("c", DataType::Int),
        ],
        vec![
            (
                "1".to_owned(),
                serde_json::json!({"a": 1, "b": "x", "c": 1}),
            ),
            (
                "2".to_owned(),
                serde_json::json!({"a": 2, "b": "y", "c": 2}),
            ),
            (
                "3".to_owned(),
                serde_json::json!({"a": 3, "b": "z", "c": 3}),
            ),
        ],
    );
    let controls = fixture.controls();
    let before = fixture.counters();

    // Act
    let batches = fixture
        .read(
            "counted",
            &["a", "b"],
            RowTraversal::CompleteSource,
            &controls,
        )
        .expect("vector read");
    let delta = fixture.counters().since(before);

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(delta.scans, 1);
    assert_eq!(delta.batches, 1);
    assert_eq!(delta.lanes, 6);
    assert_eq!(delta.values_fetched, 6);
    assert_eq!(delta.values_decoded, 6);
    assert_eq!(delta.row_documents_built, 0);
}

#[test]
fn should_count_row_documents_built_by_the_document_cursor() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table(
        "documents",
        vec![field("a", DataType::Int)],
        vec![
            ("1".to_owned(), serde_json::json!({"a": 1})),
            ("2".to_owned(), serde_json::json!({"a": 2})),
        ],
    );
    let before = fixture.counters();

    // Act
    let rows = document_values(&fixture, "documents", &["a"]);
    let delta = fixture.counters().since(before);

    // Assert
    assert_eq!(rows.len(), 2);
    assert_eq!(delta.row_documents_built, 2);
    assert_eq!(delta.batches, 0);
}

#[test]
fn should_read_only_the_selected_field_payloads() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table(
        "narrow",
        vec![
            field("a", DataType::Int),
            field("b", DataType::Text),
            field("c", DataType::Int),
        ],
        vec![
            (
                "1".to_owned(),
                serde_json::json!({"a": 1, "b": "x", "c": 1}),
            ),
            (
                "2".to_owned(),
                serde_json::json!({"a": 2, "b": "y", "c": 2}),
            ),
        ],
    );
    let controls = fixture.controls();
    let validation = fixture.midge().column_validation_reads_for_diagnostics();
    let fields = fixture.midge().column_field_reads_for_diagnostics();

    // Act
    fixture
        .read("narrow", &["b"], RowTraversal::CompleteSource, &controls)
        .expect("vector read");

    // Assert
    assert_eq!(
        fixture.midge().column_validation_reads_for_diagnostics(),
        validation
    );
    let read = fixture
        .midge()
        .column_field_reads_for_diagnostics()
        .since(fields);
    assert_eq!(read.payload_reads, 2);
}

#[test]
fn should_stop_with_cancellation_before_returning_a_batch() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table(
        "stopped",
        vec![field("a", DataType::Int)],
        vec![
            ("1".to_owned(), serde_json::json!({"a": 1})),
            ("2".to_owned(), serde_json::json!({"a": 2})),
        ],
    );
    let controls = fixture.controls();
    controls.cancellation.cancel();

    // Act
    let result = fixture.read("stopped", &["a"], RowTraversal::CompleteSource, &controls);

    // Assert
    assert!(matches!(result, Err(StorageError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn budget_fixture() -> Fixture {
    let fixture = fixture();
    fixture.table(
        "budgeted",
        vec![field("a", DataType::Int), field("b", DataType::Text)],
        vec![
            ("1".to_owned(), serde_json::json!({"a": 1, "b": "one"})),
            ("2".to_owned(), serde_json::json!({"a": 2, "b": "two"})),
            ("3".to_owned(), serde_json::json!({"a": 3, "b": "three"})),
        ],
    );
    fixture
}

fn limited(bytes: usize) -> TestRequest {
    TestRequest::new(bytes)
}

#[test]
fn should_fit_a_budget_equal_to_the_observed_peak() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = budget_fixture();
    let probe = fixture.controls();
    let batches = fixture
        .read(
            "budgeted",
            &["a", "b"],
            RowTraversal::CompleteSource,
            &probe,
        )
        .expect("probe read");
    drop(batches);
    let peak = probe.peak_query_memory_bytes();
    let controls = limited(peak);

    // Act
    let result = fixture.read(
        "budgeted",
        &["a", "b"],
        RowTraversal::CompleteSource,
        &controls,
    );

    // Assert
    assert!(result.is_ok());
}

#[test]
fn should_reject_a_budget_one_byte_below_the_observed_peak() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = budget_fixture();
    let probe = fixture.controls();
    let batches = fixture
        .read(
            "budgeted",
            &["a", "b"],
            RowTraversal::CompleteSource,
            &probe,
        )
        .expect("probe read");
    drop(batches);
    let peak = probe.peak_query_memory_bytes();
    let controls = limited(peak - 1);

    // Act
    let result = fixture.read(
        "budgeted",
        &["a", "b"],
        RowTraversal::CompleteSource,
        &controls,
    );

    // Assert
    assert!(matches!(result, Err(StorageError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bind_requested_columns_to_lanes_in_schema_order() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table(
        "bound",
        vec![field("a", DataType::Int), field("b", DataType::Text)],
        Vec::new(),
    );

    // Act
    let reader = fixture
        .midge()
        .open_column_vector_reader(
            "bound",
            &[Some("b"), None, Some("a"), Some("b")],
            RowTraversal::CompleteSource,
        )
        .expect("open")
        .expect("reader");

    // Assert
    assert_eq!(
        reader.outputs(),
        &[
            ColumnVectorOutput::Lane(1),
            ColumnVectorOutput::Identity,
            ColumnVectorOutput::Lane(0),
            ColumnVectorOutput::Lane(1),
        ]
    );
}

#[test]
fn should_decline_a_name_that_is_not_a_stored_field() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table("plain", vec![field("a", DataType::Int)], Vec::new());

    // Act
    let reader = fixture
        .midge()
        .open_column_vector_reader("plain", &[Some("missing")], RowTraversal::CompleteSource)
        .expect("open");

    // Assert
    assert!(reader.is_none());
}

#[test]
fn should_not_open_a_vector_reader_over_a_row_table() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    let schema = Schema {
        fields: vec![field("a", DataType::Int)],
    };
    let metadata = super::super::super::CollectionMeta::new("heap", None);
    fixture
        .midge
        .create_collection_with_meta("heap", &schema, &metadata)
        .expect("create row collection");

    // Act
    let reader = fixture
        .midge()
        .open_column_vector_reader("heap", &[Some("a")], RowTraversal::CompleteSource)
        .expect("open");

    // Assert
    assert!(reader.is_none());
}

#[test]
fn should_reject_a_value_whose_stored_tag_contradicts_its_type() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.table(
        "tagged",
        vec![field("a", DataType::Int)],
        vec![("1".to_owned(), serde_json::json!({"a": 1}))],
    );
    let schema = fixture.midge().row_schema("tagged").expect("row schema");
    let prefix = Midge::column_store_field_prefix(schema.relation_id, schema.fields[0].field_id);
    let stored = fixture
        .midge()
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("field entries");
    fixture
        .midge()
        .raw_put(StorageFamily::Data, &stored[0].0, b"\x05text")
        .expect("wrong-tag value");
    let controls = fixture.controls();

    // Act
    let result = fixture.read("tagged", &["a"], RowTraversal::CompleteSource, &controls);

    // Assert
    assert!(matches!(
        result.expect_err("wrong tag"),
        StorageError::Diagnostic {
            sql_state: "XX001",
            ..
        }
    ));
}

#[test]
fn should_describe_declared_types_with_the_lane_kind_the_batch_carries() {
    // Arrange
    let types = [
        DataType::SmallInt,
        DataType::Float,
        DataType::Boolean,
        DataType::Text,
        DataType::Date,
    ];

    // Act
    let kinds = types.map(|data_type| super::decode::LaneKind::of(&data_type));

    // Assert
    assert_eq!(
        kinds,
        [
            super::decode::LaneKind::Integer,
            super::decode::LaneKind::Float,
            super::decode::LaneKind::Boolean,
            super::decode::LaneKind::Utf8,
            super::decode::LaneKind::Values,
        ]
    );
}
