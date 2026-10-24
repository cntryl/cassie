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
    let statement = "temporal_sleep_helpers";
    let portal = "temporal_sleep_helpers_portal";
    let sql = "SELECT PG_SLEEP($1) AS waited, PG_SLEEP_FOR($2) AS waited_for, PG_SLEEP_UNTIL($3) AS waited_until";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT PG_SLEEP(0.0) AS waited, PG_SLEEP_FOR(INTERVAL '0 seconds') AS waited_for, PG_SLEEP_UNTIL(CURRENT_TIMESTAMP) AS waited_until",
        )]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                portal,
                statement,
                &[0],
                &[
                    Some(&b"0.0"[..]),
                    Some(&b"0 seconds"[..]),
                    Some(&b"2000-01-01 00:00:00+00"[..]),
                ],
                &[1],
            ),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

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
        vec![2278, 2278, 2278]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some(String::new()),
            Some(String::new()),
            Some(String::new())
        ]]
    );
    let described = &batches[2];
    assert_eq!(wire::error_code(described), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("sleep helper ParameterDescription")
                .1,
        ),
        vec![701, 1186, 1184]
    );
    let portal_description = &batches[3];
    assert_eq!(
        wire::error_code(portal_description),
        None,
        "sleep helper portal response: {:?}",
        error_fields(portal_description)
    );
    let binary_columns = wire::parse_row_description(
        &portal_description
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("binary sleep helper RowDescription")
            .1,
    );
    assert!(binary_columns.iter().all(|column| {
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code,
        ) == (2278, 4, -1, 1)
    }));
    assert_eq!(
        wire::data_rows(portal_description),
        vec![vec![
            Some(String::new()),
            Some(String::new()),
            Some(String::new())
        ]]
    );
}

#[test]
fn should_preserve_pg_sleep_void_identity_next_to_wildcard_projection() {
    // Arrange
    let sql = "SELECT PG_SLEEP(0.0) AS waited, * FROM (VALUES (1)) AS row(value)";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-815-sleep-with-wildcard", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let columns = wire::parse_row_description(
        &batches[0]
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("sleep and wildcard RowDescription")
            .1,
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![2278, 23]
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(String::new()), Some("1".to_owned())]]
    );
}

#[test]
fn should_preserve_pg_sleep_void_identity_through_case_projection() {
    // Arrange
    let sql = "SELECT CASE WHEN TRUE THEN PG_SLEEP(0.0) END AS waited";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-815-sleep-through-case", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let columns = wire::parse_row_description(
        &batches[0]
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("sleep CASE RowDescription")
            .1,
    );
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].type_oid, 2278);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(String::new())]]
    );
}

#[test]
fn should_preserve_pg_sleep_void_identity_through_coalesce_projection() {
    // Arrange
    let sql = "SELECT COALESCE(PG_SLEEP(0.0), PG_SLEEP(0.0)) AS waited";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-815-sleep-through-coalesce", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let columns = wire::parse_row_description(
        &batches[0]
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("sleep COALESCE RowDescription")
            .1,
    );
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].type_oid, 2278);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(String::new())]]
    );
}

#[test]
fn should_preserve_pg_sleep_void_identity_with_explicit_null_result_arms() {
    // Arrange
    let sql = "SELECT CASE WHEN TRUE THEN PG_SLEEP(0.0) ELSE NULL END AS case_waited, COALESCE(PG_SLEEP(0.0), NULL) AS coalesced_waited";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-815-sleep-with-null-results", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let columns = wire::parse_row_description(
        &batches[0]
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("sleep NULL result RowDescription")
            .1,
    );
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
fn should_keep_pg_sleep_void_identity_for_null_case_result() {
    // Arrange
    let sql = "SELECT CASE WHEN FALSE THEN PG_SLEEP(0.0) END AS case_waited";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-815-sleep-null-case-result", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let columns = wire::parse_row_description(
        &batches[0]
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("sleep NULL CASE RowDescription")
            .1,
    );
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].type_oid, 2278);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![None]]);
}

#[test]
fn should_keep_pg_sleep_void_identity_for_late_coalesce_result() {
    // Arrange
    let sql = "SELECT COALESCE(NULL, PG_SLEEP(0.0)) AS waited";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-815-sleep-late-coalesce-result", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let columns = wire::parse_row_description(
        &batches[0]
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("sleep late COALESCE RowDescription")
            .1,
    );
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].type_oid, 2278);
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
