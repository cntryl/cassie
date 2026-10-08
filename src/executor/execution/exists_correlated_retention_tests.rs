//! Actual retained enclosing-row ownership at the EXISTS scope boundary.
use super::super::Cassie;
use super::*;
use crate::types::DataType;
use std::sync::Arc;
use std::time::Instant;

struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("EXISTS fixture cleanup during unwind: {error}");
            } else {
                panic!("EXISTS fixture strict cleanup: {error}");
            }
        }
    }
}

#[test]
fn should_retain_enclosing_row_owners_for_exists_scope() {
    // Arrange
    let directory = Directory(
        std::env::temp_dir().join(format!("cassie-exists-owner-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir(&directory.0).expect("private directory");
    let cassie = Cassie::new_with_data_dir(&directory.0).expect("fixture");
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 4 * 1024 * 1024,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let statement =
        crate::sql::parse_statement("SELECT CAST(1 AS BIGINT) AS n").expect("inner SQL");
    let logical =
        super::super::build_logical_plan_in_session(&cassie, None, &statement).expect("inner plan");
    let kind = DataType::Array(Box::new(DataType::Text));
    let row = BatchRow::with_aliases(
        vec![(
            "outer.a".into(),
            crate::types::Value::Json(serde_json::json!(["x".repeat(768), null])),
        )],
        vec![("joined.a".into(), 0)],
    )
    .with_optional_data_types(Some(Arc::new(vec![kind.clone()])));
    let parent = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("actual source backing"))
            .expect("source admission"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&parent)))
        .retain_operator_memory(&controls, Arc::clone(&parent))
        .expect("source owner");

    // Act
    let ctes = super::super::CteContext::new();
    let functions = std::collections::HashMap::new();
    let context = ExistsResolutionContext {
        cassie: &cassie,
        session: None,
        cte_context: &ctes,
        user_functions: &functions,
        params: &[],
        controls: &controls,
        outer_row: Some(&row),
    };
    let retained = scoped_outer_row(&context, &logical, &row).expect("admitted outer scope");
    drop(row);
    drop(parent);

    // Assert
    assert_eq!(
        retained.get("joined.a"),
        Some(&retained.entries()[0].1),
        "qualified alias retains its original entry index"
    );
    assert!(
        retained.query_memory().is_some(),
        "EXISTS scope must retain the enclosing query owner"
    );
    assert!(
        retained.operator_memory().is_some(),
        "EXISTS scope must retain the enclosing operator owner"
    );
    assert_eq!(
        retained
            .shared_data_types()
            .expect("retained ARRAY identity")
            .as_ref(),
        &[kind]
    );
    assert!(
        controls.current_query_memory_bytes() > 0,
        "retained outer row must remain admitted"
    );
    drop(retained);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    drop(directory);
}

fn with_fixture(run: impl FnOnce(&Cassie, &LogicalPlan)) {
    let directory = Directory(
        std::env::temp_dir().join(format!("cassie-exists-admission-{}", uuid::Uuid::new_v4())),
    );
    std::fs::create_dir(&directory.0).expect("private directory");
    let cassie = Cassie::new_with_data_dir(&directory.0).expect("fixture");
    let statement =
        crate::sql::parse_statement("SELECT CAST(1 AS BIGINT) AS n").expect("inner SQL");
    let logical =
        super::super::build_logical_plan_in_session(&cassie, None, &statement).expect("inner plan");
    run(&cassie, &logical);
    drop(cassie);
    drop(directory);
}
fn limits(budget: usize) -> crate::config::CassieRuntimeLimits {
    crate::config::CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        query_timeout_ms: 0,
        ..crate::config::CassieRuntimeLimits::default()
    }
}
fn input(
    controls: &crate::runtime::QueryExecutionControls,
) -> (BatchRow, Arc<crate::runtime::QueryMemoryReservation>) {
    let row = BatchRow::with_aliases(
        vec![
            (
                "outer.a".into(),
                crate::types::Value::Json(serde_json::json!(["x".repeat(768), null])),
            ),
            (
                "outer.s".into(),
                crate::types::Value::String("y".repeat(513)),
            ),
            (
                "outer.empty".into(),
                crate::types::Value::Json(serde_json::json!([])),
            ),
        ],
        vec![
            ("joined.a".into(), 0),
            ("joined.s".into(), 1),
            ("joined.empty".into(), 2),
        ],
    )
    .with_optional_data_types(Some(Arc::new(vec![
        DataType::Array(Box::new(DataType::Text)),
        DataType::Text,
        DataType::Array(Box::new(DataType::BigInt)),
    ])));
    let parent = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("actual source backing"))
            .expect("source admission"),
    );
    (
        row.with_query_memory(Some(Arc::clone(&parent)))
            .retain_operator_memory(controls, Arc::clone(&parent))
            .expect("source owner"),
        parent,
    )
}
fn scoped(
    cassie: &Cassie,
    logical: &LogicalPlan,
    row: &BatchRow,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<BatchRow, QueryError> {
    let ctes = super::super::CteContext::new();
    let functions = std::collections::HashMap::new();
    let context = ExistsResolutionContext {
        cassie,
        session: None,
        cte_context: &ctes,
        user_functions: &functions,
        params: &[],
        controls,
        outer_row: Some(row),
    };
    scoped_outer_row(&context, logical, row)
}
#[test]
fn should_reject_scoped_copy_below_observed_admission_peak() {
    // Arrange
    with_fixture(|cassie, logical| {
        let broad = crate::runtime::QueryExecutionControls::from_limits(
            &limits(4 * 1024 * 1024),
            Instant::now(),
        );
        let (row, parent) = input(&broad);
        let source_bytes = broad.current_query_memory_bytes();
        let admitted = scoped(cassie, logical, &row, &broad).expect("measured rich scope");
        let peak = broad.peak_query_memory_bytes();
        assert!(peak > source_bytes);
        drop(admitted);
        drop(row);
        drop(parent);
        assert_eq!(broad.current_query_memory_bytes(), 0);
        let tight =
            crate::runtime::QueryExecutionControls::from_limits(&limits(peak - 1), Instant::now());
        let (row, parent) = input(&tight);

        // Act
        let rejected = scoped(cassie, logical, &row, &tight);

        // Assert
        assert!(
            matches!(
                rejected,
                Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                    _
                )))
            ),
            "{rejected:?}"
        );
        assert_eq!(
            tight.current_query_memory_bytes(),
            source_bytes,
            "failed copy retains only original input"
        );
        drop(row);
        drop(parent);
        assert_eq!(tight.current_query_memory_bytes(), 0);
        println!(
            "scoped owner input={source_bytes} peak={peak} budget={} final=0",
            peak - 1
        );
    });
}
#[test]
fn should_cancel_scoped_copy_without_losing_input_ownership() {
    // Arrange
    with_fixture(|cassie, logical| {
        let cancellation = crate::runtime::QueryCancellationHandle::new();
        let controls = crate::runtime::QueryExecutionControls::with_cancellation(
            &limits(4 * 1024 * 1024),
            Instant::now(),
            cancellation.clone(),
        );
        let (row, parent) = input(&controls);
        let bytes = controls.current_query_memory_bytes();
        cancellation.cancel();

        // Act
        let cancelled = scoped(cassie, logical, &row, &controls);

        // Assert
        assert!(
            matches!(
                cancelled,
                Err(QueryError::General(ref message)) if message == "query canceled"
            ),
            "{cancelled:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), bytes);
        drop(row);
        drop(parent);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}
