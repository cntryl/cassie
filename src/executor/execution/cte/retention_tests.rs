use super::*;
use std::time::Instant;

#[test]
fn should_retain_materialized_cte_backing_until_context_drop() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-857-{}", uuid::Uuid::new_v4()));
    let mut config = crate::config::CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 128 * 1024;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("fixture");
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let statement = crate::sql::parse_statement(
        "WITH c AS (SELECT DISTINCT CAST(NULL AS BIGINT) AS n) SELECT DISTINCT n FROM c",
    )
    .expect("CTE SQL");
    let plan =
        super::super::build_logical_plan_in_session(&cassie, None, &statement).expect("bound plan");
    let mut context = CteContext::new();
    // Act
    let output = execute_plan(
        &cassie,
        None,
        &plan,
        &mut context,
        &HashMap::new(),
        &[],
        &controls,
    )
    .expect("CTE result");
    drop(output);
    // Assert
    assert_eq!(context["c"].rows.len(), 1);
    assert_eq!(context["c"].rows[0][0].1, Value::Null);
    assert!(
        controls.current_query_memory_bytes() > 0,
        "live materialized context must retain its plain row and descriptor backing"
    );
    drop(context);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}

fn with_output(run: impl FnOnce(CteContext, Vec<BatchRow>, &QueryExecutionControls)) {
    let path = std::env::temp_dir().join(format!("cassie-857-copy-{}", uuid::Uuid::new_v4()));
    let mut config = crate::config::CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    config.limits.query_memory_budget_bytes = 128 * 1024;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("fixture");
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let statement = crate::sql::parse_statement(
        "WITH c AS (SELECT DISTINCT CAST(NULL AS BIGINT) AS n) SELECT DISTINCT n FROM c",
    )
    .expect("CTE SQL");
    let plan =
        super::super::build_logical_plan_in_session(&cassie, None, &statement).expect("bound plan");
    let mut context = CteContext::new();
    let output = execute_plan(
        &cassie,
        None,
        &plan,
        &mut context,
        &HashMap::new(),
        &[],
        &controls,
    )
    .expect("CTE result");
    run(context, output, &controls);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}

#[test]
fn should_retain_independent_cte_source_copy_after_context_drop() {
    // Arrange
    with_output(|context, output, controls| {
        assert_eq!(output.len(), 1);
        // Act
        drop(context);
        // Assert
        assert_eq!(output[0].entries()[0].1, Value::Null);
        assert!(
            controls.current_query_memory_bytes() > 0,
            "live copied output must retain independent backing"
        );
        drop(output);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

#[test]
fn should_retain_empty_cte_context_map_capacity_until_drop() {
    // Arrange
    with_output(|mut context, output, controls| {
        drop(output);
        // Act
        drop(context.remove("c"));
        // Assert
        assert!(context.is_empty());
        assert!(context.capacity() > 0);
        assert!(
            controls.current_query_memory_bytes() > 0,
            "allocated empty context map must remain admitted"
        );
        drop(context);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    });
}

std::thread_local! {
    static SEEN_OBSERVATIONS: std::cell::RefCell<Vec<(usize, usize)>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub(super) fn observe_seen(before: usize, after: usize) {
    SEEN_OBSERVATIONS.with_borrow_mut(|observations| observations.push((before, after)));
}

#[test]
fn should_admit_recursive_seen_key_growth_before_retaining_keys() {
    // Arrange
    SEEN_OBSERVATIONS.with_borrow_mut(Vec::clear);
    let path = std::env::temp_dir().join(format!("cassie-857-recursive-{}", uuid::Uuid::new_v4()));
    let mut config = crate::config::CassieRuntimeConfig::default();
    config.limits.query_timeout_ms = 0;
    let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).expect("fixture");
    let controls = QueryExecutionControls::from_limits(&config.limits, Instant::now());
    let statement = crate::sql::parse_statement("WITH RECURSIVE seq(n) AS (SELECT CAST(1 AS INT) UNION SELECT CAST(n AS INT) FROM seq) SELECT n FROM seq").expect("existing recursive SQL");
    let plan = super::super::build_logical_plan_in_session(&cassie, None, &statement)
        .expect("bound recursive plan");
    let mut context = CteContext::new();
    // Act
    let output = execute_plan(
        &cassie,
        None,
        &plan,
        &mut context,
        &HashMap::new(),
        &[],
        &controls,
    )
    .expect("duplicate stabilization");
    // Assert
    assert_eq!(output.len(), 1);
    assert_eq!(output[0].entries()[0].1, Value::Int64(1));
    SEEN_OBSERVATIONS.with_borrow(|observations| {
        assert!(
            !observations.is_empty(),
            "actual recursive seen insert must execute"
        );
        assert!(
            observations.iter().all(|(before, after)| after > before),
            "accepted seen keys must retain a new backing charge"
        );
    });
    drop(output);
    drop(context);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict fixture cleanup");
}

#[test]
fn should_admit_source_serialization_scratch_before_copy_handoff() {
    // Arrange
    let limits = crate::config::CassieRuntimeLimits {
        query_memory_budget_bytes: 8 * 1024,
        ..crate::config::CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let relation = CteRelation {
        rows: vec![vec![("n".into(), Value::String("\n".repeat(4096)))]],
        fields: vec![crate::types::FieldSchema {
            name: "n".into(),
            data_type: crate::types::DataType::Text,
            nullable: false,
        }],
        _rows_memory: None,
        _fields_memory: None,
    };
    // Act
    let result = copy_source_rows(&relation, &controls, None);
    // Assert
    assert!(
        matches!(
            result.map_err(crate::app::CassieError::from),
            Err(crate::app::CassieError::ResourceLimit(_))
        ),
        "escaped row serializer scratch must be admitted before finalization"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

std::thread_local! {
    static KEY_COPY_ADMISSION: std::cell::Cell<Option<(usize, usize)>> = const { std::cell::Cell::new(None) };
}

pub(super) fn observe_key_copy(controls: &QueryExecutionControls, name: &str) {
    KEY_COPY_ADMISSION
        .with(|observed| observed.set(Some((controls.current_query_memory_bytes(), name.len()))));
}

#[test]
fn should_admit_recursive_working_key_before_copying_name() {
    // Arrange
    KEY_COPY_ADMISSION.with(|observed| observed.set(None));
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 128,
            ..crate::config::CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let mut context = CteContext::new();
    let name = "c".repeat(4096);
    // Act
    let result = store_working_rows(&mut context, &name, &Vec::new(), &Vec::new(), &controls);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    KEY_COPY_ADMISSION.with(|observed| {
        if let Some((admitted, bytes)) = observed.get() {
            assert!(
                admitted >= bytes,
                "key copy began before admission: {admitted} < {bytes}"
            );
        }
    });
    drop(context);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_retain_copied_cte_descriptor_namespace_until_drop() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
    );
    let context = CteContext::unleased(HashMap::from([(
        "c".into(),
        CteRelation {
            rows: vec![],
            fields: vec![crate::types::FieldSchema {
                name: "n".repeat(4096),
                data_type: crate::types::DataType::Array(Box::new(crate::types::DataType::Text)),
                nullable: true,
            }],
            _rows_memory: None,
            _fields_memory: None,
        },
    )]));
    // Act
    let fields = context_fields(&context, &controls).expect("controlled namespace");
    // Assert
    assert_eq!(fields["c"][0].name.len(), 4096);
    assert!(
        controls.current_query_memory_bytes() >= 4096,
        "fresh descriptor namespace requires an independent owner"
    );
    drop(fields);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

std::thread_local! {
    static COPY_CANCELLATION: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn after_copy_row() {
    COPY_CANCELLATION.with_borrow_mut(|handle| {
        if let Some(handle) = handle.take() {
            handle.cancel();
        }
    });
}

#[test]
fn should_release_partial_context_copy_when_cancelled_after_first_row() {
    // Arrange
    let handle = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        Instant::now(),
        handle.clone(),
    );
    let rows = vec![
        vec![("n".into(), Value::String("x".repeat(4096)))],
        vec![("n".into(), Value::String("y".repeat(4096)))],
    ];
    let retained = RetainedRows::copy(&rows, &controls).expect("original backing");
    let mut context = CteContext::new();
    context
        .insert(
            "c",
            CteRelation {
                rows: retained.rows,
                fields: vec![],
                _rows_memory: Some(retained.memory),
                _fields_memory: None,
            },
            &controls,
        )
        .expect("original map");
    let before = controls.current_query_memory_bytes();
    COPY_CANCELLATION.with_borrow_mut(|target| *target = Some(handle));
    // Act
    let result = context.copy(&controls);
    // Assert
    assert!(matches!(
        result.map_err(crate::app::CassieError::from),
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(context["c"].rows, rows);
    assert_eq!(controls.current_query_memory_bytes(), before);
    assert!(controls.peak_query_memory_bytes() > before);
    drop(context);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

pub(super) fn arm_copy_cancellation(handle: crate::runtime::QueryCancellationHandle) {
    COPY_CANCELLATION.with_borrow_mut(|target| *target = Some(handle));
}
