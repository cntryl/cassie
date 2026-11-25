//! Native Boolean views retain their validated raw and metadata owner.
use super::*;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::{EncodedSource, EncodedSourceDecision};

fn fixture(controls: &QueryExecutionControls) -> (Cassie, EncodedSource) {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE boolean_owner (b BOOLEAN)", vec![])
        .expect("table");
    for (id, value) in [
        ("a", serde_json::json!(true)),
        ("b", serde_json::Value::Null),
        ("c", serde_json::json!(false)),
    ] {
        cassie
            .midge
            .put_document(
                "boolean_owner",
                Some(id.into()),
                serde_json::json!({"b":value}),
            )
            .expect("row");
    }
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX boolean_owner_cover ON boolean_owner USING column (b)",
            vec![],
        )
        .expect("index");
    let decision = cassie
        .midge
        .controlled_encoded_source("boolean_owner", &["b".into()], &controls.storage_request())
        .expect("source");
    let EncodedSourceDecision::Hit(source) = decision else {
        panic!("encoded source")
    };
    (cassie, source)
}
fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    )
}
#[test]
fn should_retain_nullable_boolean_backing_through_repeated_gather_views() {
    // Arrange
    let controls = controls();
    let (_cassie, source) = fixture(&controls);
    let field = &source.segments[0].fields[0];
    let weak = Arc::downgrade(field);
    assert_eq!(
        field.get().boolean_values(),
        Some([Some(true), None, Some(false)].as_slice())
    );
    let column =
        Column::from_encoded(&controls, &DataType::Boolean, field).expect("native Boolean");
    // Act
    let gathered = column
        .gather(&controls, &[1, 2, 1, 0])
        .expect("repeated positions");
    let slice = gathered.slice(&controls, 1, 3).expect("slice");
    let alias = slice.clone();
    drop(slice);
    drop(gathered);
    drop(column);
    drop(source);
    // Assert
    assert!(weak.upgrade().is_some());
    assert!(matches!(
        alias.cell(0).expect("false"),
        Cell::Boolean(false)
    ));
    assert!(matches!(alias.cell(1).expect("NULL"), Cell::Null));
    assert!(matches!(alias.cell(2).expect("true"), Cell::Boolean(true)));
    drop(alias);
    assert!(weak.upgrade().is_none());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
#[test]
fn should_reject_a_declared_type_mismatch_for_native_boolean_ownership() {
    // Arrange
    let controls = controls();
    let (_cassie, source) = fixture(&controls);
    let before = controls.current_query_memory_bytes();
    // Act
    let rejected =
        Column::from_encoded(&controls, &DataType::BigInt, &source.segments[0].fields[0]);
    // Assert
    assert!(rejected.is_err());
    assert_eq!(controls.current_query_memory_bytes(), before);
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
#[test]
fn should_preserve_boolean_source_ownership_when_view_admission_is_denied() {
    // Arrange
    let controls = controls();
    let (_cassie, source) = fixture(&controls);
    let blocked = controls
        .reserve_query_memory(
            controls.query_memory_budget_bytes - controls.current_query_memory_bytes(),
        )
        .expect("retain available budget");
    let before = controls.current_query_memory_bytes();
    // Act
    let rejected =
        Column::from_encoded(&controls, &DataType::Boolean, &source.segments[0].fields[0]);
    // Assert
    assert!(rejected.is_err());
    assert_eq!(controls.current_query_memory_bytes(), before);
    drop(blocked);
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
#[test]
fn should_release_native_boolean_owners_after_canceled_view_construction() {
    // Arrange
    let cancel = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
        cancel.clone(),
    );
    let (_cassie, source) = fixture(&controls);
    cancel.cancel();
    // Act
    let rejected =
        Column::from_encoded(&controls, &DataType::Boolean, &source.segments[0].fields[0]);
    // Assert
    assert!(rejected.is_err());
    drop(source);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
