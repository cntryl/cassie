use std::path::PathBuf;
use std::time::Instant;

use crate::app::{Cassie, CassieError, CassieSession};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits, ExecutionResultCacheEnabled};
use crate::midge::adapter::{
    query_scan_control_test_guard, set_query_scan_cancellation_after_entries, StorageFamily,
};
use crate::planner::logical::LogicalPlan;
use crate::runtime::{QueryExecutionControls, ReadPathSnapshot};

use super::{
    execute_ordered_column_top_k, execute_ordered_column_top_k_with_projection_probe,
    execute_ordered_row_id_page_with_projection_probe, ordered_column_heap,
    ordered_column_top_k_spec, ordered_row_id_page_spec, OrderedColumnCandidate,
    OrderedReadPathMode,
};

const COLLECTION: &str = "ordered_operator_controls";
const CANARY: &str = "ordered-second-row-decode-canary";

struct Fixture {
    cassie: Option<Cassie>,
    session: CassieSession,
    path: PathBuf,
}

impl Fixture {
    fn new(poison_first: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-ordered-active-controls-{}",
            uuid::Uuid::new_v4()
        ));
        let mut config = CassieRuntimeConfig::default();
        config.limits.query_timeout_ms = 0;
        config.limits.parallel_scan_workers = 1;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config).expect("controlled Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {COLLECTION} (payload TEXT)"),
                vec![],
            )
            .expect("RowStore table");
        cassie
            .midge
            .put_document(
                COLLECTION,
                Some("doc-0000".to_owned()),
                serde_json::json!({"payload": "x".repeat(8192)}),
            )
            .expect("oversized first row");
        cassie
            .midge
            .put_document(
                COLLECTION,
                Some("doc-0001".to_owned()),
                serde_json::json!({"payload": CANARY}),
            )
            .expect("second row");
        let (key, _) = cassie
            .midge
            .raw_scan_prefix_for_collection(COLLECTION, &[])
            .expect("RowStore entries")
            .into_iter()
            .find(|(_, value)| {
                value
                    .windows(CANARY.len())
                    .any(|bytes| bytes == CANARY.as_bytes())
            })
            .expect("unique second-row payload");
        cassie
            .midge
            .raw_put(StorageFamily::Data, &key, &[u8::MAX])
            .expect("poison second row");
        assert!(matches!(
            cassie.midge.get_document(COLLECTION, "doc-0001"),
            Err(CassieError::Parse(_))
        ));
        if poison_first {
            let payload = "x".repeat(8192);
            let (key, mut value) = cassie
                .midge
                .raw_scan_prefix_for_collection(COLLECTION, &[])
                .expect("RowStore entries")
                .into_iter()
                .find(|(_, value)| {
                    value
                        .windows(payload.len())
                        .any(|bytes| bytes == payload.as_bytes())
                })
                .expect("wide first-row payload");
            let payload_start = value
                .windows(payload.len())
                .position(|bytes| bytes == payload.as_bytes())
                .expect("encoded first-row string");
            value[payload_start] = u8::MAX;
            cassie
                .midge
                .raw_put(StorageFamily::Data, &key, &value)
                .expect("invalid first-row UTF-8 canary");
            assert!(matches!(
                cassie.midge.get_document(COLLECTION, "doc-0000"),
                Err(CassieError::Parse(_))
            ));
        }
        Self {
            cassie: Some(cassie),
            session,
            path,
        }
    }

    fn cassie(&self) -> &Cassie {
        self.cassie.as_ref().expect("live fixture")
    }

    fn plan(&self, suffix: &str) -> LogicalPlan {
        self.plan_sql(&format!("SELECT _id, payload FROM {COLLECTION} {suffix}"))
    }

    fn plan_sql(&self, sql: &str) -> LogicalPlan {
        let statement = crate::sql::parse_statement(sql).expect("ordered statement");
        super::super::build_logical_plan_in_session(self.cassie(), Some(&self.session), &statement)
            .expect("ordered logical plan")
    }

    fn remove_second_row_canary(&self) {
        let (key, _) = self
            .cassie()
            .midge
            .raw_scan_prefix_for_collection(COLLECTION, &[])
            .expect("fixture entries")
            .into_iter()
            .find(|(_, value)| value.as_slice() == [u8::MAX])
            .expect("unique encoded second-row canary");
        self.cassie()
            .midge
            .raw_delete(StorageFamily::Data, &key)
            .expect("single valid row for column-top-k selection");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.cassie.take());
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn successful_ordered_paths(paths: &ReadPathSnapshot) -> [u64; 5] {
    [
        paths.ordered_scans,
        paths.storage_top_k_scans,
        paths.keyset_scans,
        paths.degraded_offset_scans,
        paths.heap_top_k_scans,
    ]
}

fn assert_selected_mode(plan: &LogicalPlan, expected: &str) {
    let spec = ordered_row_id_page_spec(plan, &[]).expect("row-ID shortcut must be eligible");
    let actual = match spec.read_path_mode() {
        OrderedReadPathMode::StorageTopK => "storage_top_k",
        OrderedReadPathMode::Keyset => "keyset",
        OrderedReadPathMode::DegradedOffset => "degraded_offset",
    };
    assert_eq!(actual, expected);
}

#[test]
fn should_cancel_ordered_row_id_pages_at_the_first_controlled_read() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new(false);
    for (suffix, mode) in [
        ("ORDER BY _id LIMIT 2", "storage_top_k"),
        ("WHERE _id >= 'doc-0000' ORDER BY _id LIMIT 2", "keyset"),
        ("ORDER BY _id LIMIT 1 OFFSET 1", "degraded_offset"),
    ] {
        let plan = fixture.plan(suffix);
        assert_selected_mode(&plan, mode);
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_timeout_ms: 0,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        assert!(controls.deadline.is_none());
        assert!(!controls.is_cancelled());
        super::super::check_timeout(&controls).expect("live query admission");
        let before_reads = fixture.cassie().midge.query_scan_entries_for_diagnostics();
        let before_paths = fixture.cassie().runtime.snapshot().read_paths;
        set_query_scan_cancellation_after_entries(Some(1));

        // Act
        let result = execute_ordered_column_top_k(
            fixture.cassie(),
            Some(&fixture.session),
            &[],
            &plan,
            &controls,
        )
        .map_err(CassieError::from);
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert!(
            matches!(result, Err(CassieError::QueryCancelled)),
            "{mode} must cancel before the second-row decode canary: {result:?}"
        );
        assert_eq!(
            fixture.cassie().midge.query_scan_entries_for_diagnostics() - before_reads,
            1
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
        assert_eq!(
            successful_ordered_paths(&fixture.cassie().runtime.snapshot().read_paths),
            successful_ordered_paths(&before_paths)
        );
    }
}

#[test]
fn should_reject_ordered_row_id_pages_with_a_tiny_preflight_budget() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new(true);
    for (suffix, mode) in [
        ("ORDER BY _id LIMIT 2", "storage_top_k"),
        ("WHERE _id >= 'doc-0000' ORDER BY _id LIMIT 2", "keyset"),
        ("ORDER BY _id LIMIT 1 OFFSET 1", "degraded_offset"),
    ] {
        let plan = fixture.plan(suffix);
        assert_selected_mode(&plan, mode);
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_timeout_ms: 0,
                query_memory_budget_bytes: 512,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        assert!(controls.deadline.is_none());
        assert!(!controls.is_cancelled());
        super::super::check_timeout(&controls).expect("live query admission");
        let before_reads = fixture.cassie().midge.query_scan_entries_for_diagnostics();
        let before_paths = fixture.cassie().runtime.snapshot().read_paths;
        set_query_scan_cancellation_after_entries(Some(2));

        // Act
        let result = execute_ordered_column_top_k(
            fixture.cassie(),
            Some(&fixture.session),
            &[],
            &plan,
            &controls,
        )
        .map_err(CassieError::from);
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert!(
            matches!(result, Err(CassieError::ResourceLimit(_))),
            "{mode} must reserve the first row before the second read: {result:?}"
        );
        assert!(fixture.cassie().midge.query_scan_entries_for_diagnostics() - before_reads <= 1);
        assert_eq!(controls.current_query_memory_bytes(), 0);
        assert_eq!(
            successful_ordered_paths(&fixture.cassie().runtime.snapshot().read_paths),
            successful_ordered_paths(&before_paths)
        );
    }
}

#[test]
fn should_reserve_ordered_row_id_pages_before_decoding_the_first_payload() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new(true);
    let limits = CassieRuntimeLimits {
        query_timeout_ms: 0,
        query_memory_budget_bytes: 4 * 1024,
        ..CassieRuntimeLimits::default()
    };
    let header_plan = fixture.plan("WHERE _id > 'doc-0001' ORDER BY _id LIMIT 1");
    let header_controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let header_rows = execute_ordered_column_top_k(
        fixture.cassie(),
        Some(&fixture.session),
        &[],
        &header_plan,
        &header_controls,
    )
    .expect("the same metadata must fit before reading a row")
    .expect("ordered shortcut remains selected");
    assert!(header_rows.is_empty());
    assert_eq!(header_controls.current_query_memory_bytes(), 0);
    for (suffix, mode) in [
        ("ORDER BY _id LIMIT 2", "storage_top_k"),
        ("WHERE _id >= 'doc-0000' ORDER BY _id LIMIT 2", "keyset"),
        ("ORDER BY _id LIMIT 1 OFFSET 1", "degraded_offset"),
    ] {
        let plan = fixture.plan(suffix);
        assert_selected_mode(&plan, mode);
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        assert!(controls.deadline.is_none());
        assert!(!controls.is_cancelled());
        super::super::check_timeout(&controls).expect("live query admission");
        let before_reads = fixture.cassie().midge.query_scan_entries_for_diagnostics();
        let before_paths = fixture.cassie().runtime.snapshot().read_paths;
        set_query_scan_cancellation_after_entries(Some(2));

        // Act
        let result = execute_ordered_column_top_k(
            fixture.cassie(),
            Some(&fixture.session),
            &[],
            &plan,
            &controls,
        )
        .map_err(CassieError::from);
        set_query_scan_cancellation_after_entries(None);

        // Assert
        assert!(
            matches!(result, Err(CassieError::ResourceLimit(_))),
            "{mode} must reserve the first row before its UTF-8 canary: {result:?}"
        );
        assert_eq!(
            fixture.cassie().midge.query_scan_entries_for_diagnostics() - before_reads,
            1
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
        assert_eq!(
            successful_ordered_paths(&fixture.cassie().runtime.snapshot().read_paths),
            successful_ordered_paths(&before_paths)
        );
    }
}

#[test]
fn should_reserve_repeated_ordered_row_id_projections_before_building_output() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new(false);
    let plan = fixture.plan_sql(&format!(
        "SELECT payload AS a, payload AS b, payload AS c, payload AS d \
         FROM {COLLECTION} ORDER BY _id LIMIT 1"
    ));
    assert_selected_mode(&plan, "storage_top_k");
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_timeout_ms: 0,
            query_memory_budget_bytes: 48 * 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    super::super::check_timeout(&controls).expect("live query admission");
    let before_reads = fixture.cassie().midge.query_scan_entries_for_diagnostics();
    let before_paths = fixture.cassie().runtime.snapshot().read_paths;
    let mut builders = 0;

    // Act
    let result = execute_ordered_row_id_page_with_projection_probe(
        fixture.cassie(),
        Some(&fixture.session),
        &[],
        &plan,
        &controls,
        || {
            builders += 1;
            Err(CassieError::Execution(
                "ordered projection builder canary".to_owned(),
            ))
        },
    )
    .map_err(CassieError::from);

    // Assert
    assert!(
        matches!(result, Err(CassieError::ResourceLimit(_))),
        "temporary and final copies must be reserved before construction: {result:?}"
    );
    assert_eq!(builders, 0);
    assert_eq!(
        fixture.cassie().midge.query_scan_entries_for_diagnostics() - before_reads,
        1
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(
        successful_ordered_paths(&fixture.cassie().runtime.snapshot().read_paths),
        successful_ordered_paths(&before_paths)
    );
}

#[test]
fn should_reserve_repeated_ordered_column_projections_before_building_output() {
    // Arrange
    let _guard = query_scan_control_test_guard();
    let fixture = Fixture::new(false);
    fixture.remove_second_row_canary();
    let plan = fixture.plan_sql(&format!(
        "SELECT payload AS a, payload AS b, payload AS c, payload AS d \
         FROM {COLLECTION} ORDER BY payload LIMIT 1"
    ));
    assert!(ordered_row_id_page_spec(&plan, &[]).is_none());
    assert!(ordered_column_top_k_spec(&plan).is_some());
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_timeout_ms: 0,
            query_memory_budget_bytes: 48 * 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    super::super::check_timeout(&controls).expect("live query admission");
    let before_reads = fixture.cassie().midge.query_scan_entries_for_diagnostics();
    let before_paths = fixture.cassie().runtime.snapshot().read_paths;
    let mut builders = 0;

    // Act
    let result = execute_ordered_column_top_k_with_projection_probe(
        fixture.cassie(),
        Some(&fixture.session),
        &[],
        &plan,
        &controls,
        || {
            builders += 1;
            Err(CassieError::Execution(
                "ordered projection builder canary".to_owned(),
            ))
        },
    )
    .map_err(CassieError::from);

    // Assert
    assert!(
        matches!(result, Err(CassieError::ResourceLimit(_))),
        "temporary and final copies must be reserved before construction: {result:?}"
    );
    assert_eq!(builders, 0);
    assert_eq!(
        fixture.cassie().midge.query_scan_entries_for_diagnostics() - before_reads,
        1
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(
        successful_ordered_paths(&fixture.cassie().runtime.snapshot().read_paths),
        successful_ordered_paths(&before_paths)
    );
}

#[test]
fn should_reserve_ordered_column_heap_capacity_before_retaining_candidates() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_timeout_ms: 0,
            query_memory_budget_bytes: 512,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    super::super::check_timeout(&controls).expect("live heap admission");

    // Act
    let (heap, memory) = ordered_column_heap(&controls)
        .expect("an empty input need not retain the requested window");

    // Assert
    let backing_bytes = heap.capacity() * std::mem::size_of::<OrderedColumnCandidate>();
    assert!(
        backing_bytes <= controls.current_query_memory_bytes(),
        "retained heap backing {backing_bytes} exceeds its live reservation {}",
        controls.current_query_memory_bytes()
    );
    assert!(controls.current_query_memory_bytes() <= controls.query_memory_budget_bytes);
    drop((heap, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
