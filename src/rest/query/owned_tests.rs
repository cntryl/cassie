use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::executor::{ColumnMeta, QueryResult};
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

use super::RestQueryResult;

const BUDGET: usize = 128 * 1024;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: BUDGET,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn result(controls: &QueryExecutionControls) -> QueryResult {
    let mut builder = QueryResult::try_builder(controls).expect("result builder admission");
    builder
        .try_push_column(&ColumnMeta::text("message"))
        .expect("column admission");
    builder
        .try_push_row(&[Value::String("retained payload".repeat(128))])
        .expect("row admission");
    builder.try_set_command("SELECT 1").expect("command admission");
    builder.finish().expect("managed result")
}

#[test]
fn should_keep_converted_rest_payload_reserved_after_source_consumption() {
    // Arrange
    let controls = controls();
    let source = result(&controls);

    // Act
    let output = RestQueryResult::try_from(source).expect("REST conversion");
    let live = controls.current_query_memory_bytes();
    let literal = serde_json::to_value(&output).expect("REST serialization");
    drop(output);
    let released = controls.current_query_memory_bytes();

    // Assert
    assert!(live > 0, "converted storage must retain its memory reservation");
    assert_eq!(literal["columns"][0]["name"], "message");
    assert_eq!(literal["rows"][0][0], "retained payload".repeat(128));
    assert_eq!(literal["command"], "SELECT 1");
    assert_eq!(released, 0);
}

#[test]
fn should_charge_rest_conversion_to_the_admitted_requesting_view() {
    // Arrange
    let producer = controls();
    let requester = controls();
    let source = result(&producer);
    let producer_before = producer.current_query_memory_bytes();
    let view = source.try_clone_for_query(&requester).expect("request admission");

    // Act
    let output = RestQueryResult::try_from(view).expect("REST conversion");
    let requester_live = requester.current_query_memory_bytes();
    let producer_after = producer.current_query_memory_bytes();
    drop(output);
    let requester_released = requester.current_query_memory_bytes();
    drop(source);
    let producer_released = producer.current_query_memory_bytes();

    // Assert
    assert!(requester_live > 0, "the converted payload belongs to this requesting query");
    assert_eq!(producer_after, producer_before);
    assert_eq!(requester_released, 0);
    assert_eq!(producer_released, 0);
}

#[test]
fn should_reject_rest_conversion_before_exceeding_the_shared_budget() {
    // Arrange
    let producer = controls();
    let requester = controls();
    let source = result(&producer);
    let view = source.try_clone_for_query(&requester).expect("request admission");
    let blocker = requester
        .reserve_query_memory(BUDGET - requester.current_query_memory_bytes())
        .expect("occupy remaining shared budget");

    // Act
    let converted = RestQueryResult::try_from(view);
    let rejected = matches!(converted, Err(CassieError::ResourceLimit(_)));
    drop(converted);
    let source_preserved = source.rows()[0][0] == Value::String("retained payload".repeat(128));
    drop(blocker);
    let requester_released = requester.current_query_memory_bytes();
    drop(source);
    let producer_released = producer.current_query_memory_bytes();

    // Assert
    assert!(rejected, "required JSON allocation must fail when no budget remains");
    assert!(source_preserved);
    assert_eq!(requester_released, 0);
    assert_eq!(producer_released, 0);
}
