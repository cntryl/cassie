use super::reserve_filter_rows;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

#[test]
fn should_admit_copy_where_directory_while_decoded_rows_are_retained() {
    // Arrange
    let slot_bytes = std::mem::size_of::<Vec<Option<String>>>();
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 2 * slot_bytes,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls
        .reserve_query_memory(slot_bytes)
        .expect("input directory");
    // Act
    let (directory, memory) = reserve_filter_rows(1, &controls).expect("output directory");
    // Assert
    assert_eq!(controls.current_query_memory_bytes(), 2 * slot_bytes);
    drop((directory, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_copy_where_output_before_allocating_when_input_uses_budget() {
    // Arrange
    let slot_bytes = std::mem::size_of::<Vec<Option<String>>>();
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: slot_bytes,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls
        .reserve_query_memory(slot_bytes)
        .expect("input directory");
    // Act
    let result = reserve_filter_rows(1, &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), slot_bytes);
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_copy_where_string_parameter_before_copying() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 3,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let result = super::copy_parameter(&crate::app::CopyColumn::RowId, Some("four"), &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_copy_where_while_predicate_is_executing() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE copy_where_cancel (id INT)", vec![])
        .expect("table");
    let parsed = crate::sql::parser::parse_statement("COPY copy_where_cancel (id) FROM STDIN")
        .expect("parse");
    let crate::sql::ast::QueryStatement::Copy(statement) = parsed.statement else {
        panic!("COPY")
    };
    let (_pause_guard, selected, resume) = crate::runtime::pause_next_query_at_sleep_entry();
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let worker = std::thread::spawn(move || {
        super::filter_rows(
            &cassie,
            &session,
            &statement,
            vec![vec![Some("1".into())]],
            "id > 0 AND pg_sleep(0) IS NULL",
            &controls,
        )
    });
    selected
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("predicate entered sleep");
    // Act
    cancellation.cancel();
    resume.send(()).expect("resume predicate");
    let outcome = worker.join().expect("worker");
    // Assert
    assert!(matches!(
        outcome,
        Err(crate::app::CassieError::QueryCancelled)
    ));
}

#[test]
fn should_use_originating_copy_deadline_for_where_execution() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let mut controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    controls.deadline = Some(Instant::now());
    // Act
    let outcome = cassie.execute_copy_where_with_controls(&session, "SELECT 1", vec![], &controls);
    // Assert
    assert!(matches!(
        outcome,
        Err(crate::app::CassieError::DeadlineExceeded)
    ));
}

#[test]
fn should_use_originating_copy_budget_for_where_execution() {
    // Arrange
    let cassie = crate::app::Cassie::new_in_memory_with_config(
        crate::config::CassieRuntimeConfig::default(),
    )
    .expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(1).expect("input");
    // Act
    let outcome = cassie.execute_copy_where_with_controls(&session, "SELECT 1", vec![], &controls);
    // Assert
    assert!(matches!(
        outcome,
        Err(crate::app::CassieError::ResourceLimit(_))
    ));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bind_quoted_copy_where_column_as_parameter() {
    // Arrange
    let columns = ["Id"];
    // Act
    let expression = super::parameterize_columns("\"Id\" > 1", &columns);
    // Assert
    assert_eq!(expression.expect("quoted parameter"), "$1 > 1");
}

#[test]
fn should_respect_query_admission_during_copy_where_evaluation() {
    // Arrange
    let mut config = crate::config::CassieRuntimeConfig::default();
    config.limits.max_query_workers = 1;
    let cassie = crate::app::Cassie::new_in_memory_with_config(config).expect("Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let occupied = cassie
        .runtime
        .try_begin_running_query()
        .expect("occupied slot");
    // Act
    let denied = cassie.execute_copy_where_with_controls(&session, "SELECT 1", vec![], &controls);
    drop(occupied);
    let admitted = cassie.execute_copy_where_with_controls(&session, "SELECT 1", vec![], &controls);
    // Assert
    assert!(
        matches!(denied, Err(crate::app::CassieError::Execution(message)) if message == "query admission exhausted")
    );
    assert!(admitted.is_ok());
}

#[test]
fn should_bind_copy_where_quoted_column_with_escaped_quote() {
    // Arrange
    let columns = ["a\"b"];
    // Act
    let expression = super::parameterize_columns("\"a\"\"b\" = 1", &columns);
    // Assert
    assert_eq!(expression.expect("escaped quoted parameter"), "$1 = 1");
}
