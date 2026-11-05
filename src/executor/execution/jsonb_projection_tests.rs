use super::*;
use crate::config::CassieRuntimeLimits;
use crate::executor::execution::build_logical_plan;
use crate::types::DataType;
use std::sync::Arc;
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}
fn project(controls: &QueryExecutionControls) -> Result<Vec<Batch>, QueryError> {
    let outer = BatchRow::new(vec![(
        "outer_docs.payload".into(),
        Value::Json(serde_json::json!({"n":1})),
    )])
    .with_optional_data_types(Some(Arc::new(vec![DataType::Jsonb])));
    let origin = Arc::new(controls.reserve_query_memory(outer.unleased_body_bytes()?)?);
    let outer = outer
        .with_query_memory(Some(Arc::clone(&origin)))
        .retain_operator_memory(controls, Arc::clone(&origin))?;
    let local = BatchRow::new(Vec::new())
        .with_outer_scope(Arc::new(outer))
        .retain_operator_memory(controls, origin)?;
    let statement = crate::sql::parse_statement("SELECT 1 AS copied").expect("plan fixture");
    let mut plan = build_logical_plan(&crate::catalog::Catalog::new(), &statement)?;
    plan.projection = vec![crate::sql::SelectItem::Column {
        name: "outer_docs.payload".into(),
        alias: Some("copied".into()),
    }];
    apply_projection_phase(
        vec![vec![local]],
        &plan,
        &[],
        None,
        &HashMap::new(),
        None,
        controls,
    )
}

#[test]
fn should_retain_projected_outer_jsonb_metadata_through_the_last_row_alias() {
    // Arrange
    let controls = controls(1_048_576);
    // Act
    let output = project(&controls).expect("outer JSONB projection");
    let alias = output[0][0].clone();
    drop(output);
    let retained = controls.current_query_memory_bytes();
    // Assert
    assert_eq!(alias.data_types(), [DataType::Jsonb]);
    assert_eq!(
        alias.get("copied"),
        Some(&Value::Json(serde_json::json!({"n":1})))
    );
    assert!(retained > 0);
    drop(alias);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_outer_jsonb_projection_at_the_measured_boundary() {
    // Arrange
    let ample = controls(1_048_576);
    let measured = project(&ample).expect("measured projection");
    let peak = ample.peak_query_memory_bytes();
    drop(measured);
    let exact = controls(peak);
    let scarce = controls(peak - 1);
    // Act
    let admitted = project(&exact);
    let denied = project(&scarce);
    // Assert
    assert!(admitted.is_ok());
    assert!(matches!(
        denied,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    drop(admitted);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(scarce.current_query_memory_bytes(), 0);
}
