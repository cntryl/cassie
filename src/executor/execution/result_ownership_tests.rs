use std::time::Instant;

use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

use super::{ColumnMeta, QueryResult};

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn result(controls: &QueryExecutionControls) -> QueryResult {
    let mut builder = QueryResult::try_builder(controls).expect("admitted builder");
    builder
        .try_push_column(&ColumnMeta::text("variable_width"))
        .expect("column");
    builder
        .try_push_row(&[Value::String("retained result".repeat(64))])
        .expect("row");
    builder.try_set_command("SELECT").expect("command");
    builder.finish().expect("admitted result")
}

#[test]
fn should_keep_result_memory_reserved_until_the_last_shared_owner_drops() {
    // Arrange
    let controls = controls(1_048_576);
    let result = result(&controls);

    // Act
    let retained = controls.current_query_memory_bytes();
    let alias = result.clone();
    drop(result);

    // Assert
    assert!(
        retained > 896,
        "the variable-width payload must remain admitted"
    );
    assert_eq!(controls.current_query_memory_bytes(), retained);
    drop(alias);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_share_result_storage_without_allocating_a_deep_clone() {
    // Arrange
    let controls = controls(1_048_576);
    let result = result(&controls);
    let peak = controls.peak_query_memory_bytes();

    // Act
    let alias = result.clone();

    // Assert
    assert_eq!(result.rows().as_ptr(), alias.rows().as_ptr());
    assert_eq!(controls.peak_query_memory_bytes(), peak);
}

#[test]
fn should_keep_admitted_directory_charged_after_row_construction_is_denied() {
    // Arrange
    let calibration = controls(1_048_576);
    let builder = QueryResult::try_builder(&calibration).expect("calibration");
    let header = calibration.current_query_memory_bytes();
    drop(builder);
    let directory = 4 * std::mem::size_of::<Vec<Value>>();
    let controls = controls(header + directory + std::mem::size_of::<Value>() - 1);
    let mut builder = QueryResult::try_builder(&controls).expect("header");

    // Act
    let denied = builder.try_push_row(&[Value::Int64(7)]);
    let live = controls.current_query_memory_bytes();
    let result = builder
        .finish()
        .expect("empty result preserves its directory");
    let empty = result.rows().is_empty();
    drop(result);

    // Assert
    assert!(matches!(
        denied,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert!(empty);
    assert_eq!(live, header + directory);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_an_exact_fitting_result_without_losing_its_capacity_lease() {
    // Arrange
    let calibration = controls(1_048_576);
    let builder = QueryResult::try_builder(&calibration).expect("calibration");
    let header = calibration.current_query_memory_bytes();
    drop(builder);
    let bytes = header + 4 * std::mem::size_of::<Vec<Value>>() + std::mem::size_of::<Value>();
    let controls = controls(bytes);
    let mut builder = QueryResult::try_builder(&controls).expect("header");

    // Act
    builder
        .try_push_row(&[Value::Int64(7)])
        .expect("exact fitting row");
    let result = builder.finish().expect("result");
    let retained = controls.current_query_memory_bytes();
    let row = result.rows()[0][0].clone();
    drop(result);

    // Assert
    assert_eq!(row, Value::Int64(7));
    assert_eq!(retained, bytes);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_existing_rows_when_overlapping_directory_growth_is_denied() {
    // Arrange
    let calibration = controls(1_048_576);
    let builder = QueryResult::try_builder(&calibration).expect("calibration");
    let header = calibration.current_query_memory_bytes();
    drop(builder);
    let baseline =
        header + 4 * std::mem::size_of::<Vec<Value>>() + 4 * std::mem::size_of::<Value>();
    let budget = baseline + 8 * std::mem::size_of::<Vec<Value>>() - 1;
    let controls = controls(budget);
    let mut builder = QueryResult::try_builder(&controls).expect("header");
    for value in 0..4 {
        builder
            .try_push_row(&[Value::Int64(value)])
            .expect("original row");
    }

    // Act
    let denied = builder.try_push_row(&[Value::Int64(4)]);
    let live = controls.current_query_memory_bytes();
    let result = builder.finish().expect("preserved result");
    let preserved = result
        .rows()
        .iter()
        .enumerate()
        .all(|(index, row)| row == &[Value::Int64(i64::try_from(index).expect("small index"))]);
    let count = result.row_count();
    drop(result);

    // Assert
    assert!(matches!(
        denied,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    assert!(preserved);
    assert_eq!(count, 4);
    assert_eq!(live, baseline);
    assert_eq!(controls.peak_query_memory_bytes(), baseline);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[derive(Clone)]
struct InterruptingValues<'a> {
    values: std::slice::Iter<'a, Value>,
    visits: &'a std::cell::Cell<usize>,
    cancellation: &'a crate::runtime::QueryCancellationHandle,
}

impl<'a> Iterator for InterruptingValues<'a> {
    type Item = &'a Value;

    fn next(&mut self) -> Option<Self::Item> {
        let value = self.values.next()?;
        let visits = self.visits.get() + 1;
        self.visits.set(visits);
        if visits == 2 {
            self.cancellation.cancel();
        }
        Some(value)
    }
}

#[test]
fn should_cancel_during_admission_prepass_before_allocating_row_capacity() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1_048_576,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
        cancellation.clone(),
    );
    let mut builder = QueryResult::try_builder(&controls).expect("header");
    let header = controls.current_query_memory_bytes();
    let visits = std::cell::Cell::new(0);
    let values: [Value; 100] = std::array::from_fn(|_| Value::Int64(1));
    let values = InterruptingValues {
        values: values.iter(),
        visits: &visits,
        cancellation: &cancellation,
    };

    // Act
    let denied = builder.try_push_values(values);
    let live = controls.current_query_memory_bytes();
    drop(builder);

    // Assert
    assert!(matches!(
        denied,
        Err(crate::app::CassieError::QueryCancelled)
    ));
    assert_eq!(
        visits.get(),
        2,
        "stop visiting the input at the cancellation frontier"
    );
    assert_eq!(live, header);
    assert_eq!(controls.peak_query_memory_bytes(), header);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reuse_preadmitted_command_capacity_when_the_remaining_budget_is_exhausted() {
    // Arrange
    let controls = controls(1_048_576);
    let mut builder = QueryResult::try_builder(&controls).expect("header");
    builder
        .try_set_command("DELETE 18446744073709551615")
        .expect("maximum count admission");
    let blocker = controls
        .reserve_query_memory(1_048_576 - controls.current_query_memory_bytes())
        .expect("occupy remaining budget");
    let peak = controls.peak_query_memory_bytes();

    // Act
    let changed = builder.try_set_command("DELETE 1");
    let result = builder.finish().expect("completed owner");
    let actual = result.command().to_owned();
    drop(result);
    drop(blocker);

    // Assert
    assert!(
        changed.is_ok(),
        "finalize a preadmitted counted tag without allocating a replacement"
    );
    assert_eq!(actual, "DELETE 1");
    assert_eq!(controls.peak_query_memory_bytes(), peak);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reuse_physical_admission_when_a_requester_view_returns_to_its_producer() {
    // Arrange
    let producer = controls(1_048_576);
    let requester = controls(1_048_576);
    let result = result(&producer);
    let view = result
        .try_clone_for_query(&requester)
        .expect("requester exposure");
    let producer_bytes = producer.current_query_memory_bytes();
    let requester_bytes = requester.current_query_memory_bytes();
    let blocker = producer
        .reserve_query_memory(1_048_576 - producer_bytes)
        .expect("fill producer remaining budget");

    // Act
    let returned = view.try_clone_for_query(&producer);
    let producer_live = producer.current_query_memory_bytes();
    drop(blocker);

    // Assert
    let returned = returned.expect("physical producer already owns the shared result");
    assert_eq!(producer_live, 1_048_576);
    assert_eq!(returned.rows().as_ptr(), result.rows().as_ptr());
    assert_eq!(requester.current_query_memory_bytes(), requester_bytes);
    let conversion = returned
        .admission()
        .try_sibling(123)
        .expect("producer conversion");
    assert_eq!(producer.current_query_memory_bytes(), producer_bytes + 123);
    assert_eq!(requester.current_query_memory_bytes(), requester_bytes);
    drop((conversion, returned, result, view));
    assert_eq!(producer.current_query_memory_bytes(), 0);
    assert_eq!(requester.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_source_at_the_metadata_replacement_overlap_budget_frontier() {
    // Arrange
    let columns = [ColumnMeta::text("replacement metadata".repeat(64))];
    let calibration = controls(1_048_576);
    let original = result(&calibration);
    let replacement = original
        .try_with_columns(&calibration, &columns)
        .expect("calibrate complete overlapping owners");
    let exact = calibration.peak_query_memory_bytes();
    assert_eq!(calibration.current_query_memory_bytes(), exact);
    drop((replacement, original));
    assert_eq!(calibration.current_query_memory_bytes(), 0);

    for budget in [exact - 1, exact] {
        let controls = controls(budget);
        let original = result(&controls);
        let original_bytes = controls.current_query_memory_bytes();
        let original_rows = original.rows().to_vec();
        let original_columns = original.columns().to_vec();

        // Act
        let replacement = original.try_with_columns(&controls, &columns);

        // Assert
        assert_eq!(original.rows(), original_rows);
        assert_eq!(original.columns(), original_columns);
        assert_eq!(original.command(), "SELECT");
        assert!(controls.peak_query_memory_bytes() <= budget);
        if budget == exact {
            let replacement = replacement.expect("exact complete overlap must fit");
            assert_eq!(replacement.columns(), columns);
            assert_eq!(replacement.rows(), original_rows);
            assert_eq!(controls.current_query_memory_bytes(), exact);
            drop(original);
            assert_eq!(
                controls.current_query_memory_bytes(),
                exact - original_bytes
            );
            drop(replacement);
        } else {
            let error = replacement.expect_err("one-byte-short overlap must fail");
            assert_eq!(error.descriptor().sql_state, "54000");
            assert_eq!(controls.current_query_memory_bytes(), original_bytes);
            drop(original);
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_keep_object_identifier_oids_when_only_column_metadata_is_rebuilt() {
    // Arrange
    let controls = controls(1_048_576);
    let mut builder = QueryResult::try_builder(&controls).expect("admitted builder");
    builder
        .try_push_column(&ColumnMeta::from_data_type(
            "relation",
            &crate::types::DataType::ObjectId(crate::types::ObjectIdKind::RegClass),
        ))
        .expect("column");
    builder
        .try_push_row(&[Value::String("item".to_owned())])
        .expect("row");
    builder
        .try_set_object_oids(0, vec![Some(16_385)])
        .expect("object identities");
    builder.try_set_command("SELECT").expect("command");
    let result = builder.finish().expect("admitted result");
    let renamed = vec![ColumnMeta::from_data_type(
        "renamed",
        &crate::types::DataType::ObjectId(crate::types::ObjectIdKind::RegClass),
    )];

    // Act
    let rebuilt = result
        .try_with_columns(&controls, &renamed)
        .expect("rebuilt columns");
    let replaced_rows = result
        .try_with_rows(&controls, &[vec![Value::String("other".to_owned())]])
        .expect("rebuilt rows");

    // Assert
    assert_eq!(rebuilt.object_oid(0, 0), Some(16_385));
    assert_eq!(replaced_rows.object_oid(0, 0), None);
}
