use super::{run_script, wire, Step};

fn error_fields(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<(char, String)>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .collect()
}

#[test]
fn should_describe_pg_sleep_float8_result_as_void() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT PG_SLEEP(0.0) AS waited",
    )])];

    // Act
    let batches = run_script("issue-815-temporal-sleep-result-types", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "sleep helper ErrorResponse fields: {:?}",
        error_fields(&batches[0])
    );
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("temporal sleep helper RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![2278]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(String::new())]]
    );
}

#[test]
fn should_recompute_wall_clock_for_each_prepared_statement_execution() {
    // Arrange
    let statement = "clock_timestamp_reuse";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET TIME ZONE 'UTC'")]),
        Step::ParseFlush(wire::parse_frame(
            statement,
            "SELECT CLOCK_TIMESTAMP() AS observed_at",
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("first_clock_portal", statement, &[], &[], &[]),
            wire::describe_portal_frame("first_clock_portal"),
            wire::execute_frame("first_clock_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 1")]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("second_clock_portal", statement, &[], &[], &[]),
            wire::describe_portal_frame("second_clock_portal"),
            wire::execute_frame("second_clock_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-815-prepared-clock-timestamp-volatility", steps);

    // Assert
    let errors = batches
        .iter()
        .enumerate()
        .filter(|(_, batch)| wire::error_code(batch).is_some())
        .map(|(index, batch)| (index, error_fields(batch)))
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "prepared clock ErrorResponse fields by batch: {errors:?}"
    );
    let statement_description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("prepared clock statement RowDescription");
    let statement_column = &wire::parse_row_description(&statement_description.1)[0];
    assert_eq!(
        (
            statement_column.type_oid,
            statement_column.type_size,
            statement_column.type_mod,
        ),
        (1184, 8, -1)
    );

    let first_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("first clock portal RowDescription");
    let second_description = batches[5]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("second clock portal RowDescription");
    for description in [first_description, second_description] {
        let column = &wire::parse_row_description(&description.1)[0];
        assert_eq!((column.type_oid, column.format_code), (1184, 0));
    }

    let first_rows = wire::data_rows(&batches[3]);
    let second_rows = wire::data_rows(&batches[5]);
    let first_value = first_rows[0][0].as_ref().expect("first wall-clock value");
    let second_value = second_rows[0][0].as_ref().expect("second wall-clock value");
    assert_ne!(
        first_value, second_value,
        "reusing a prepared statement must evaluate clock_timestamp at each execution"
    );
}

#[test]
fn should_evaluate_wall_clock_for_each_fixture_table_row() {
    // Arrange
    let table = "issue_815_clock_observation_rows";
    let values = (1..=1_000)
        .map(|id| format!("({id})"))
        .collect::<Vec<_>>()
        .join(", ");
    let insert = format!("INSERT INTO {table} (id) VALUES {values}");
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_815_clock_observation_rows (id INT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(&insert)]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CLOCK_TIMESTAMP() AS observed_at \
             FROM issue_815_clock_observation_rows ORDER BY id",
        )]),
    ];

    // Act
    let batches = run_script("issue-815-wall-clock-table-volatility", steps);

    // Assert
    let setup_errors = batches[..2]
        .iter()
        .enumerate()
        .filter(|(_, batch)| wire::error_code(batch).is_some())
        .map(|(index, batch)| (index, error_fields(batch)))
        .collect::<Vec<_>>();
    assert!(
        setup_errors.is_empty(),
        "clock fixture setup ErrorResponse fields by batch: {setup_errors:?}"
    );
    let result = &batches[2];
    assert_eq!(
        wire::error_code(result),
        None,
        "row-wise clock ErrorResponse fields: {:?}",
        error_fields(result)
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("row-wise clock volatility RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (1184, 8, -1)
    );
    let rows = wire::data_rows(result);
    assert_eq!(rows.len(), 1_000);
    let first = rows[0][0]
        .as_deref()
        .expect("first row has a clock_timestamp value");
    assert!(
        rows.iter()
            .skip(1)
            .filter_map(|row| row[0].as_deref())
            .any(|observed_at| observed_at != first),
        "clock_timestamp must be evaluated for the generated rows"
    );
}
