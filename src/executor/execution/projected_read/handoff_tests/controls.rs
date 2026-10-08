//! Capacity and control laws for the finite specialized projection handoff.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use crate::types::DataType;

fn run(budget: usize, cancel: bool, empty: bool, array: bool) -> (usize, bool) {
    let path =
        std::env::temp_dir().join(format!("cassie-handoff-controls-{}", uuid::Uuid::new_v4()));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    let statement =
        crate::sql::parse_statement("WITH c AS (SELECT CAST(1 AS BIGINT) AS n) SELECT n FROM c")
            .expect("SQL");
    let mut plan = super::super::super::build_logical_plan_in_session(&cassie, None, &statement)
        .expect("private seam plan");
    if empty {
        plan.limit = Some(Expr::IntegerLiteral(0));
    }
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
        cancellation.clone(),
    );
    let kind = (0..9).fold(DataType::Text, |kind, _| DataType::Array(Box::new(kind)));
    let rows = (0..6)
        .map(|index| {
            let value = if array {
                Value::Null
            } else {
                Value::Int64(index)
            };
            let row = batch::BatchRow::new(vec![("n".into(), value)]);
            if array {
                row.with_optional_data_types(Some(Arc::new(vec![kind.clone()])))
            } else {
                row
            }
        })
        .collect::<Vec<_>>();
    let body = rows
        .iter()
        .map(|row| row.unleased_body_bytes().expect("source body"))
        .sum();
    let parent = Arc::new(controls.reserve_query_memory(body).expect("source lease"));
    let mut rows = rows
        .into_iter()
        .map(|row| {
            row.with_query_memory(Some(Arc::clone(&parent)))
                .retain_operator_memory(&controls, Arc::clone(&parent))
                .expect("source root")
        })
        .collect::<Vec<_>>();
    let tail = rows.split_off(5);
    let mut batches = vec![rows, tail];
    let functions = HashMap::new();
    if cancel {
        cancellation.cancel();
    }
    let result = finalize_projected_filtered_read(
        ProjectedReadFinalization {
            cassie: &cassie,
            session: None,
            plan: &plan,
            user_functions: &functions,
            params: &[],
            controls: &controls,
            apply_filter: false,
            apply_sort: false,
            index_usage: None,
        },
        &mut batches,
    );
    drop(parent);
    let succeeded = result.is_ok();
    match result {
        Ok(output) => {
            if empty {
                assert!(output.is_empty());
                assert_eq!(
                    output.capacity(),
                    0,
                    "empty output cannot hold row-attached charge"
                );
            } else {
                assert_eq!(output.len(), 6);
                assert_eq!(output.capacity(), 10, "5+1 flatten growth witness");
                for (index, row) in output.iter().enumerate() {
                    assert_eq!(
                        row.get("n"),
                        Some(&if array {
                            Value::Null
                        } else {
                            Value::Int64(i64::try_from(index).expect("index"))
                        })
                    );
                    if array {
                        assert_eq!(row.data_types(), &[kind.clone()]);
                    }
                }
                let actual = output
                    .iter()
                    .map(|row| row.owned_body_bytes().expect("warmed body"))
                    .sum::<usize>()
                    + output.capacity() * std::mem::size_of::<BatchRow>();
                assert!(
                    controls.current_query_memory_bytes() >= actual,
                    "retained charge must cover actual output capacity and warmed lazy lookups"
                );
            }
            drop(output);
        }
        Err(error) if cancel => assert!(
            matches!(error, QueryError::General(ref message) if message == "query canceled")
        ),
        Err(error) => assert!(
            matches!(
                error,
                QueryError::Cassie(crate::app::CassieError::ResourceLimit(_))
            ),
            "controlled denial: {error:?}"
        ),
    }
    drop(batches);
    let peak = controls.peak_query_memory_bytes();
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict cleanup");
    (peak, succeeded)
}

#[test]
fn should_retain_non_power_of_two_output_capacity_and_lazy_lookups() {
    // Arrange
    let budget = 4 * 1024 * 1024;
    // Act
    let (_, succeeded) = run(budget, false, false, false);
    // Assert
    assert!(succeeded);
}

#[test]
fn should_retain_recursive_array_null_descriptors_after_handoff() {
    // Arrange
    let budget = 4 * 1024 * 1024;
    // Act
    let (_, succeeded) = run(budget, false, false, true);
    // Assert
    assert!(succeeded);
}

#[test]
fn should_deny_owned_projection_before_exceeding_its_measured_peak() {
    // Arrange
    let (peak, _) = run(4 * 1024 * 1024, false, false, false);
    // Act
    let (_, succeeded) = run(peak - 1, false, false, false);
    // Assert
    assert!(!succeeded);
}

#[test]
fn should_cancel_owned_projection_without_returning_rows_or_retained_charge() {
    // Arrange
    let budget = 4 * 1024 * 1024;
    // Act
    let (_, succeeded) = run(budget, true, false, false);
    // Assert
    assert!(!succeeded);
}

#[test]
fn should_release_allocated_batches_when_projection_pagination_returns_empty() {
    // Arrange
    let budget = 4 * 1024 * 1024;
    // Act
    let (_, succeeded) = run(budget, false, true, false);
    // Assert
    assert!(succeeded);
}

#[test]
fn should_release_fresh_borrowed_projection_buffers_before_cancelled_guard_drops() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-handoff-late-cancel-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie = Cassie::new_with_data_dir(&path).expect("fixture");
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 4 * 1024 * 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
        cancellation.clone(),
    );
    let row = BatchRow::new(vec![("n".into(), Value::String("s".repeat(65536)))]);
    let parent = Arc::new(
        controls
            .reserve_query_memory(row.unleased_body_bytes().expect("source body"))
            .expect("source"),
    );
    let row = row
        .with_query_memory(Some(Arc::clone(&parent)))
        .retain_operator_memory(&controls, Arc::clone(&parent))
        .expect("prior root");
    let input = vec![vec![row]];
    let projection = (0..17)
        .map(|index| SelectItem::Column {
            name: "n".into(),
            alias: Some(format!("x{index}")),
        })
        .collect::<Vec<_>>();
    let (memory, scratch) = super::super::super::projection_handoff::ProjectionOutputMemory::admit(
        &controls,
        &input,
        &projection,
        true,
        false,
    )
    .expect("preadmission");
    let mut projected =
        projection::project_batches(input, &projection, &[], None, &HashMap::new(), None)
            .expect("fresh copies");
    cancellation.cancel();
    // Act
    let result = memory.retain(&controls, &mut projected);
    let body = projected
        .iter()
        .map(|batch| {
            batch
                .iter()
                .map(|row| row.owned_body_bytes().expect("body"))
                .sum::<usize>()
                + batch.capacity() * std::mem::size_of::<BatchRow>()
        })
        .sum::<usize>()
        + projected.capacity() * std::mem::size_of::<batch::Batch>();
    let current = controls.current_query_memory_bytes();
    let cancelled =
        matches!(&result, Err(QueryError::General(message)) if message == "query canceled");
    let released = projected.is_empty() && projected.capacity() == 0;
    println!("late_cancel current={current} still_live_body={body} outer_capacity={} released={released}", projected.capacity());
    drop(result);
    drop(scratch);
    drop(projected);
    drop(parent);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    drop(cassie);
    std::fs::remove_dir_all(path).expect("strict cleanup");
    // Assert
    assert!(cancelled, "existing cancellation error is preserved");
    assert!(
        released || current >= body,
        "fresh borrowed buffers outlive their admission: {current} < {body}"
    );
}
