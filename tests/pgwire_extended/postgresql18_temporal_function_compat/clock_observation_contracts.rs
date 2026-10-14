use super::{run_script, wire, Step};

#[test]
fn should_describe_temporal_sleep_helpers_as_void() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT PG_SLEEP_FOR(INTERVAL '0 seconds') AS waited_for, \
                PG_SLEEP_UNTIL(CLOCK_TIMESTAMP()) AS waited_until",
    )])];

    // Act
    let batches = run_script("issue-815-temporal-sleep-result-types", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
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
        vec![2278, 2278]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(String::new()), Some(String::new())]]
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
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT PG_SLEEP_FOR(INTERVAL '50 milliseconds')",
        )]),
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
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
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
