use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use crate::app::Cassie;
use crate::catalog::{canonical_relation_name, CollectionCardinalityStats};
use crate::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled, OperatorSwitchingEnabled};
use crate::runtime::QueryExecutionControls;

use super::{
    side_selection, try_execute_indexed_bounded_inner_join_with_context,
    try_execute_streaming_bounded_inner_join_with_context, BatchRow, Expr, JoinExecutionSpec,
    JoinKind, JoinRetentionContext, JoinRetentionPhase, QueryError, QuerySource,
    SourceExecutionEnv,
};
use crate::sql::ast::BinaryOp;

const LEFT: &str = "bounded_retention_left";
const RIGHT: &str = "bounded_retention_right";
const KEY_BYTES: usize = 32;
const PAYLOAD_BYTES: usize = 64;

#[derive(Clone, Copy, Debug)]
enum Path {
    IndexedLeft,
    IndexedRight,
    BuildLeft,
    BuildRight,
    Dense,
    Sample,
}

struct Fixture {
    cassie: Option<Cassie>,
    config: CassieRuntimeConfig,
    path: PathBuf,
    left: String,
    right: String,
}

impl Fixture {
    fn new(selected: Path) -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-bounded-join-retention-{}",
            uuid::Uuid::new_v4()
        ));
        let mut config = CassieRuntimeConfig::default();
        config.limits.query_timeout_ms = 0;
        config.limits.query_memory_budget_bytes = 64 * 1_024;
        config.limits.vectorized_joins_enabled = true;
        config.limits.vectorized_join_batch_size = if matches!(selected, Path::Dense) {
            128
        } else {
            1
        };
        config.limits.adaptive_execution_enabled = false;
        config.limits.operator_switching_enabled = OperatorSwitchingEnabled::disabled();
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone())
            .expect("controlled join fixture");
        cassie.startup().expect("bootstrap canonical join catalog");
        let session = cassie.create_session("root", None);
        let left = canonical_relation_name("postgres", "public", LEFT);
        let right = canonical_relation_name("postgres", "public", RIGHT);
        let (left_count, right_count) = match selected {
            Path::BuildLeft => (1, 4),
            Path::BuildRight => (4, 1),
            _ => (1, 1),
        };
        for (table, collection, count) in [(LEFT, &left, left_count), (RIGHT, &right, right_count)]
        {
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE {table} (join_key TEXT NOT NULL, payload TEXT)"),
                    vec![],
                )
                .expect("join collection");
            for index in 0..count {
                cassie
                    .midge
                    .put_document(
                        collection,
                        Some(format!("row-{index}")),
                        serde_json::json!({
                            "join_key": "k".repeat(KEY_BYTES),
                            "payload": "p".repeat(PAYLOAD_BYTES),
                        }),
                    )
                    .expect("narrow join input");
            }
            cassie.catalog.hydrate_cardinality_stats(
                collection,
                CollectionCardinalityStats {
                    row_count: u64::try_from(count).expect("fixture cardinality"),
                    ..CollectionCardinalityStats::default()
                },
            );
            let rows = cassie
                .execute_sql(&session, &format!("SELECT * FROM {table}"), vec![])
                .expect("the complete matching full source must fit");
            assert_eq!(rows.rows.len(), count);
            assert_eq!(
                cassie
                    .runtime
                    .snapshot()
                    .query
                    .current_accounted_memory_bytes,
                0
            );
        }
        let indexed = match selected {
            Path::IndexedLeft => Some(LEFT),
            Path::IndexedRight => Some(RIGHT),
            _ => None,
        };
        if let Some(table) = indexed {
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE INDEX {table}_idx ON {table} (join_key)"),
                    vec![],
                )
                .expect("the sole scalar join index pins the indexed side");
        }
        Self {
            cassie: Some(cassie),
            config,
            path,
            left,
            right,
        }
    }

    fn cassie(&self) -> &Cassie {
        self.cassie.as_ref().expect("live join fixture")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.cassie.take();
        std::fs::remove_dir_all(&self.path).expect("remove bounded join fixture");
    }
}

fn probe_phase_admission(selected: Path, phase: JoinRetentionPhase) -> Result<usize, QueryError> {
    let fixture = Fixture::new(selected);
    let cassie = fixture.cassie();
    let controls = QueryExecutionControls::from_limits(&fixture.config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };
    let entry_bytes = Cell::new(0);
    let entered = Cell::new(0);
    let constructed = Cell::new(0);
    let minimum = match phase {
        JoinRetentionPhase::BoundedBuild | JoinRetentionPhase::BoundedSampleKey => KEY_BYTES,
        JoinRetentionPhase::BoundedOutput => 2 * PAYLOAD_BYTES,
        _ => PAYLOAD_BYTES,
    };
    let entry = |entered_phase| {
        if entered_phase == phase {
            entered.set(entered.get() + 1);
            entry_bytes.set(controls.current_query_memory_bytes());
        }
    };
    let constructor = |constructed_phase| {
        if constructed_phase == phase {
            constructed.set(constructed.get() + 1);
            let required = entry_bytes.get() + minimum;
            if controls.current_query_memory_bytes() < required {
                return Err(QueryError::General(format!(
                    "bounded {selected:?} {phase:?} constructor reached without admitting its owned copy"
                )));
            }
        }
        Ok(())
    };
    let retention = JoinRetentionContext::with_entry_probe(&entry, &constructor);
    let before = cassie.runtime.snapshot();
    let result = execute_selected(&env, &fixture, selected, &retention);

    assert!(
        entered.get() > 0,
        "the target phase must activate: {selected:?} {phase:?}"
    );
    assert!(
        constructed.get() > 0,
        "the narrow fixture must reach its constructor"
    );
    let rows = match result {
        Ok(rows) => rows,
        Err(error) => {
            assert_eq!(controls.current_query_memory_bytes(), 0);
            assert_eq!(
                cassie.runtime.snapshot().joins.executions,
                before.joins.executions
            );
            return Err(error);
        }
    };
    assert!(!rows.is_empty());
    let row_count = rows.len();
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let metrics = cassie.runtime.snapshot();
    let reason = match selected {
        Path::BuildLeft => Some("left_build_hydrated_row_count"),
        Path::BuildRight => Some("right_build_kept_close_estimate"),
        Path::Dense => Some("dense_stream_preemptive_temp_budget"),
        _ => None,
    };
    if let Some(reason) = reason {
        assert_eq!(metrics.joins.last_bounded_side_selection_reason, reason);
    }
    if matches!(selected, Path::IndexedLeft | Path::IndexedRight) {
        let indexed = if matches!(selected, Path::IndexedLeft) {
            &fixture.left
        } else {
            &fixture.right
        };
        assert_eq!(metrics.read_paths.last_index_scan_collection, *indexed);
        assert!(metrics.read_paths.index_seek_scans > 0);
    }
    Ok(row_count)
}

fn execute_selected(
    env: &SourceExecutionEnv<'_>,
    fixture: &Fixture,
    selected: Path,
    retention: &JoinRetentionContext<'_>,
) -> Result<Vec<BatchRow>, QueryError> {
    if matches!(selected, Path::Sample) {
        let fields = vec!["join_key".to_owned(), "payload".to_owned()];
        let keys = side_selection::sample_join_keys_with_context(
            env,
            &fixture.left,
            &format!("{}.join_key", fixture.left),
            &fields,
            1,
            retention,
        )?;
        assert_eq!(keys.len(), 1);
        return Ok(vec![BatchRow::new(Vec::new())]);
    }
    let left = QuerySource::Collection(
        crate::sql::IdentifierPath::parse(&fixture.left).expect("left collection path"),
    );
    let right = QuerySource::Collection(
        crate::sql::IdentifierPath::parse(&fixture.right).expect("right collection path"),
    );
    let on = Expr::Binary {
        left: Box::new(Expr::Column(format!("{}.join_key", fixture.left))),
        op: BinaryOp::Eq,
        right: Box::new(Expr::Column(format!("{}.join_key", fixture.right))),
    };
    let spec = JoinExecutionSpec {
        source: &crate::sql::QuerySource::SingleRow,
        left: &left,
        right: &right,
        kind: JoinKind::Inner,
        on: &on,
        outer_row: None,
        row_budget: Some(1),
    };
    assert!(env.cassie.runtime.limits().vectorized_joins_enabled);
    assert!(!env
        .cassie
        .runtime
        .limits()
        .operator_switching_enabled
        .is_enabled());
    let left_columns = super::collection_join_columns(env, &fixture.left)
        .expect("the left fixture must have catalog metadata");
    let right_columns = super::collection_join_columns(env, &fixture.right)
        .expect("the right fixture must have catalog metadata");
    let keys = super::merge_join_keys(&on, &left_columns, &right_columns).unwrap_or_else(|| {
        panic!("fixture equality must resolve: {on:?}; {left_columns:?}; {right_columns:?}")
    });
    if matches!(selected, Path::IndexedLeft | Path::IndexedRight) {
        assert!(super::indexed_join_plan(env, &fixture.left, &fixture.right, &keys).is_some());
    } else {
        assert!(
            super::streaming_join_spec(env, &left, &right, JoinKind::Inner, &on, Some(1)).is_some()
        );
    }
    let rows = if matches!(selected, Path::IndexedLeft | Path::IndexedRight) {
        try_execute_indexed_bounded_inner_join_with_context(env, &spec, retention)?
    } else {
        try_execute_streaming_bounded_inner_join_with_context(
            env,
            &spec,
            &mut super::CteContext::new(),
            retention,
        )?
    };
    let joined = rows.expect("the pinned bounded implementation must execute");
    let (batches, _text) = super::super::finish_retained_join(env, joined)?;
    Ok(crate::executor::batch::flatten_batches(batches))
}

#[test]
fn should_admit_indexed_left_stream_source_before_conversion() {
    // Arrange
    let selected = Path::IndexedLeft;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedSource);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_indexed_right_point_row_before_conversion() {
    // Arrange
    let selected = Path::IndexedRight;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedIndexedPoint);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_indexed_left_output_before_combining_rows() {
    // Arrange
    let selected = Path::IndexedLeft;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedOutput);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_indexed_right_output_before_combining_rows() {
    // Arrange
    let selected = Path::IndexedRight;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedOutput);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_left_build_stream_source_before_conversion() {
    // Arrange
    let selected = Path::BuildLeft;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedSource);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_right_build_stream_source_before_conversion() {
    // Arrange
    let selected = Path::BuildRight;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedSource);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_left_build_state_before_constructing_join_keys() {
    // Arrange
    let selected = Path::BuildLeft;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedBuild);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_right_build_state_before_constructing_join_keys() {
    // Arrange
    let selected = Path::BuildRight;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedBuild);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_left_build_stream_output_before_combining_rows() {
    // Arrange
    let selected = Path::BuildLeft;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedOutput);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_right_build_stream_output_before_combining_rows() {
    // Arrange
    let selected = Path::BuildRight;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedOutput);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_dense_stream_sources_before_conversion() {
    // Arrange
    let selected = Path::Dense;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedSource);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_dense_stream_output_before_combining_rows() {
    // Arrange
    let selected = Path::Dense;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedOutput);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}

#[test]
fn should_admit_bounded_side_selection_keys_before_copying_them() {
    // Arrange
    let selected = Path::Sample;
    // Act
    let result = probe_phase_admission(selected, JoinRetentionPhase::BoundedSampleKey);
    // Assert
    assert!(
        result.is_ok(),
        "retained phase admission is required: {result:?}"
    );
}
