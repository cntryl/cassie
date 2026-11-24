//! Vector reader tests (#1358 S2).

use super::{ColumnVectorBatch, ColumnVectorOutput, COLUMN_VECTOR_BATCH_ROWS};
use crate::app::{Cassie, CassieError, CassieSession};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::adapter::{
    query_scan_control_test_guard, RowDecode, RowTraversal, StorageFamily,
};
use crate::adapter::{ColumnVectorReadSnapshot, Midge};
use crate::runtime::QueryExecutionControls;
use cassie_core::types::lanes::{LaneValidity, RawLanes};
use cassie_core::types::{DataType, Value};
use std::time::Instant;

const ALL_TYPES: &str = "s SMALLINT, i INT, b BIGINT, f FLOAT, t BOOLEAN, x TEXT, v VARCHAR(8), \
    c CHAR(4), d DATE, ts TIMESTAMP, u UUID, j JSON, jb JSONB, n NUMERIC(10,2)";
const ALL_TYPE_FIELDS: [&str; 14] = [
    "s", "i", "b", "f", "t", "x", "v", "c", "d", "ts", "u", "j", "jb", "n",
];

struct Fixture {
    cassie: Cassie,
    session: CassieSession,
}

fn fixture() -> Fixture {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::from_env().expect("config"))
            .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    Fixture { cassie, session }
}

impl Fixture {
    fn run(&self, sql: &str) {
        self.cassie
            .execute_sql(&self.session, sql, vec![])
            .unwrap_or_else(|error| panic!("{sql}: {error}"));
    }

    fn controls(&self) -> QueryExecutionControls {
        QueryExecutionControls::from_limits(&self.cassie.runtime.limits(), Instant::now())
    }

    fn midge(&self) -> &Midge {
        &self.cassie.midge
    }

    fn read(
        &self,
        table: &str,
        fields: &[&str],
        traversal: RowTraversal,
        controls: &QueryExecutionControls,
    ) -> Result<Vec<ColumnVectorBatch>, CassieError> {
        let requested = fields.iter().map(|field| Some(*field)).collect::<Vec<_>>();
        let mut reader = self
            .midge()
            .open_column_vector_reader(table, &requested, traversal)?
            .expect("column table reader");
        let mut batches = Vec::new();
        while let Some(batch) =
            reader.next_batch(self.midge(), COLUMN_VECTOR_BATCH_ROWS, controls)?
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
            .next_documents(fixture.midge(), 64, &controls)
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
                    document.payload.get(*name).map_or(Value::Null, |json| {
                        crate::executor::scan::try_json_to_typed_value(json, data_type)
                            .expect("conversion")
                    })
                })
                .collect();
            rows.push((document.id, values));
        }
    }
    rows
}

fn lanes_by_row(batches: &[ColumnVectorBatch], fields: usize) -> Vec<(String, Vec<Value>)> {
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

fn typed_fixture() -> Fixture {
    let fixture = fixture();
    fixture.run(&format!(
        "CREATE TABLE typed ({ALL_TYPES}) WITH (storage = column_store)"
    ));
    fixture.run(
        "INSERT INTO typed VALUES \
         (1, 70000, 9007199254740993, 1.5, true, 'alpha', 'beta', 'ab', '2024-01-02', \
          '2024-01-02 03:04:05', '00000000-0000-0000-0000-000000000001', '{\"a\":[1,2]}', \
          '{\"b\":null}', 12.34), \
         (NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL), \
         (-2, -70000, -9007199254740993, -0.25, false, '', 'é', 'cd', '1999-12-31', \
          '1999-12-31 23:59:59', '00000000-0000-0000-0000-000000000002', 'null', '[]', 0.00)",
    );
    fixture
}

#[test]
fn should_decode_every_type_into_the_value_the_document_path_builds() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = typed_fixture();
    let controls = fixture.controls();

    // Act
    let batches = fixture
        .read(
            "typed",
            &ALL_TYPE_FIELDS,
            RowTraversal::CompleteSource,
            &controls,
        )
        .expect("vector read");

    // Assert
    let mut vector = lanes_by_row(&batches, ALL_TYPE_FIELDS.len());
    let mut documents = document_values(&fixture, "typed", &ALL_TYPE_FIELDS);
    vector.sort_by(|left, right| left.0.cmp(&right.0));
    documents.sort_by(|left, right| left.0.cmp(&right.0));
    assert_eq!(vector.len(), 3);
    assert_eq!(vector, documents);
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
    fixture.run("CREATE TABLE packed (note TEXT) WITH (storage = column_store)");
    let long = "abc".repeat(200);
    fixture.run(&format!("INSERT INTO packed VALUES ('{long}'), ('short')"));
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
        .read("typed", &["b"], RowTraversal::CompleteSource, &controls)
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
    fixture.run("CREATE TABLE grown (a INT) WITH (storage = column_store)");
    fixture.run("INSERT INTO grown VALUES (1), (2)");
    fixture.run("ALTER TABLE grown ADD COLUMN b INT");
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
    fixture.run("CREATE TABLE holes (a INT) WITH (storage = column_store)");
    fixture.run("INSERT INTO holes VALUES (1), (2)");
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
    assert_eq!(
        result.expect_err("missing required field").sql_state(),
        "XX001"
    );
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
    fixture.run(&format!(
        "CREATE TABLE {name} (a TEXT) WITH (storage = column_store)"
    ));
    fixture.run(&format!("INSERT INTO {name} VALUES ('m'), ('n')"));
    plant_orphan(&fixture, name, id);
    fixture
}

#[test]
fn should_reject_an_orphan_between_markers_on_a_complete_scan() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.run("CREATE TABLE between_orphan (a TEXT) WITH (storage = column_store)");
    fixture.run("INSERT INTO between_orphan VALUES ('m'), ('n')");
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
    assert_eq!(result.expect_err("orphan").sql_state(), "XX001");
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
    assert_eq!(result.expect_err("orphan").sql_state(), "XX001");
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
    fixture.run("CREATE TABLE wide (a INT) WITH (storage = column_store)");
    fixture.run(&format!(
        "INSERT INTO wide SELECT n FROM generate_series(1, {}) AS g(n)",
        COLUMN_VECTOR_BATCH_ROWS + 1
    ));
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
    fixture.run("CREATE TABLE counted (a INT, b TEXT, c INT) WITH (storage = column_store)");
    fixture.run("INSERT INTO counted VALUES (1,'x',1), (2,'y',2), (3,'z',3)");
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
    fixture.run("CREATE TABLE documents (a INT) WITH (storage = column_store)");
    fixture.run("INSERT INTO documents VALUES (1), (2)");
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
    fixture.run("CREATE TABLE narrow (a INT, b TEXT, c INT) WITH (storage = column_store)");
    fixture.run("INSERT INTO narrow VALUES (1,'x',1), (2,'y',2)");
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
    fixture.run("CREATE TABLE stopped (a INT) WITH (storage = column_store)");
    fixture.run("INSERT INTO stopped VALUES (1), (2)");
    let handle = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        handle.clone(),
    );
    handle.cancel();

    // Act
    let result = fixture.read("stopped", &["a"], RowTraversal::CompleteSource, &controls);

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

fn budget_fixture() -> Fixture {
    let fixture = fixture();
    fixture.run("CREATE TABLE budgeted (a INT, b TEXT) WITH (storage = column_store)");
    fixture.run("INSERT INTO budgeted VALUES (1,'one'), (2,'two'), (3,'three')");
    fixture
}

fn limited(bytes: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: bytes,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
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
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bind_requested_columns_to_lanes_in_schema_order() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = fixture();
    fixture.run("CREATE TABLE bound (a INT, b TEXT) WITH (storage = column_store)");

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
    fixture.run("CREATE TABLE plain (a INT) WITH (storage = column_store)");

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
    fixture.run("CREATE TABLE heap (a INT)");

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
    fixture.run("CREATE TABLE tagged (a INT) WITH (storage = column_store)");
    fixture.run("INSERT INTO tagged VALUES (1)");
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
    assert_eq!(result.expect_err("wrong tag").sql_state(), "XX001");
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
