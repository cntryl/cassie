//! PostgreSQL 18 window-function values and result identities over pgwire (#806).

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn run_window_query(label: &str, sql: &str) -> Vec<(u8, Vec<u8>)> {
    let batches = run_script(
        label,
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_806_window_wire (id INT PRIMARY KEY, grp TEXT, n INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_806_window_wire VALUES (1,'a',10),(2,'a',10),(3,'a',30),(4,'b',7)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        ],
    );
    assert_eq!(wire::error_code(&batches[0]), None, "window fixture CREATE");
    assert_eq!(wire::error_code(&batches[1]), None, "window fixture INSERT");
    batches[2].clone()
}

fn assert_window_rows(
    frames: &[(u8, Vec<u8>)],
    expected_oids: &[i32],
    expected_rows: &[Vec<Option<String>>],
) {
    let error = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload));
    assert_eq!(
        wire::error_code(frames),
        None,
        "window query ErrorResponse fields: {error:?}"
    );
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("window query RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        expected_oids
    );
    assert_eq!(wire::data_rows(frames), expected_rows);
}

#[test]
fn should_report_ntile_partition_buckets_with_int4_identity() {
    // Arrange
    let sql = "SELECT id, ntile(2) OVER (PARTITION BY grp ORDER BY n) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-ntile-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 23],
        &[
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), Some("1".to_owned())],
            vec![Some("3".to_owned()), Some("2".to_owned())],
            vec![Some("4".to_owned()), Some("1".to_owned())],
        ],
    );
}

#[test]
fn should_report_percent_rank_peer_values_with_float8_identity() {
    // Arrange
    let sql = "SELECT id, percent_rank() OVER (PARTITION BY grp ORDER BY n) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-percent-rank-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 701],
        &[
            vec![Some("1".to_owned()), Some("0".to_owned())],
            vec![Some("2".to_owned()), Some("0".to_owned())],
            vec![Some("3".to_owned()), Some("1".to_owned())],
            vec![Some("4".to_owned()), Some("0".to_owned())],
        ],
    );
}

#[test]
fn should_report_cume_dist_peer_values_with_float8_identity() {
    // Arrange
    let sql = "SELECT id, cume_dist() OVER (PARTITION BY grp ORDER BY n) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-cume-dist-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 701],
        &[
            vec![Some("1".to_owned()), Some("0.6666666666666666".to_owned())],
            vec![Some("2".to_owned()), Some("0.6666666666666666".to_owned())],
            vec![Some("3".to_owned()), Some("1".to_owned())],
            vec![Some("4".to_owned()), Some("1".to_owned())],
        ],
    );
}

#[test]
fn should_infer_ntile_bucket_parameters_as_int4() {
    // Arrange
    let statement = "ntile_parameter";
    let batches = run_script(
        "issue-806-window-ntile-parameter-oid",
        vec![
            Step::ParseFlush(wire::parse_frame(statement, "SELECT ntile($1) OVER ()")),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];

    // Assert
    assert_eq!(wire::error_code(parsed), None);
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("NTILE ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [23]);
    let description = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("NTILE RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
}

#[test]
fn should_infer_integer_window_arguments_as_int4() {
    // Arrange
    let statements = [
        ("lag_offset", "SELECT lag(42, $1) OVER ()"),
        ("lead_offset", "SELECT lead(42, $1) OVER ()"),
        ("nth_position", "SELECT nth_value(42, $1) OVER ()"),
    ];
    let mut steps = Vec::with_capacity(statements.len() * 2);
    for (statement, sql) in statements {
        steps.push(Step::ParseFlush(wire::parse_frame(statement, sql)));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("issue-806-window-offset-parameter-oid", steps);

    // Assert
    for index in 0..statements.len() {
        let parsed = &batches[index * 2];
        let described = &batches[index * 2 + 1];
        let parse_error = parsed
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload));
        assert_eq!(
            wire::error_code(parsed),
            None,
            "Parse case {index}: {parse_error:?}"
        );
        assert_eq!(wire::error_code(described), None, "Describe case {index}");
        let parameters = described
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("window ParameterDescription");
        assert_eq!(wire::parse_parameter_description(&parameters.1), [23]);
        let description = described
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("window RowDescription");
        assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    }
}

#[test]
fn should_infer_window_default_parameters_as_int4() {
    // Arrange
    let statements = [
        ("lag_default", "SELECT lag(42, 1, $1) OVER ()"),
        ("lead_default", "SELECT lead(42, 1, $1) OVER ()"),
    ];
    let mut steps = Vec::with_capacity(statements.len() * 2);
    for (statement, sql) in statements {
        steps.push(Step::ParseFlush(wire::parse_frame(statement, sql)));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("issue-806-window-default-parameter-oid", steps);

    // Assert
    for index in 0..statements.len() {
        let parsed = &batches[index * 2];
        let described = &batches[index * 2 + 1];
        let parse_error = parsed
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload));
        assert_eq!(
            wire::error_code(parsed),
            None,
            "Parse case {index}: {parse_error:?}"
        );
        assert_eq!(wire::error_code(described), None, "Describe case {index}");
        let parameters = described
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("window ParameterDescription");
        assert_eq!(wire::parse_parameter_description(&parameters.1), [23]);
        let description = described
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("window RowDescription");
        assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    }
}

#[test]
fn should_infer_lag_lead_parameters_from_explicit_default_types() {
    // Arrange
    let statements = [
        ("lag_value_from_default", "SELECT lag($1, $2, $3) OVER ()"),
        ("lead_value_from_default", "SELECT lead($1, $2, $3) OVER ()"),
    ];
    let mut steps = Vec::with_capacity(statements.len() * 2);
    for (statement, sql) in statements {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            sql,
            &[0, 0, 23],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]));
    }

    let batches = run_script("issue-806-window-value-from-default-oid", steps);

    // Act
    for index in 0..statements.len() {
        let parse = &batches[index * 2];
        let described = &batches[index * 2 + 1];
        let parse_error = parse
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload));

        // Assert
        assert_eq!(
            wire::error_code(parse),
            None,
            "Parse case {index}: {parse_error:?}"
        );
        assert_eq!(wire::error_code(described), None, "Describe case {index}");
        let parameters = described
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("window ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1),
            [23, 23, 23]
        );
        let description = described
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("window RowDescription");
        assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
    }
}

#[test]
fn should_resolve_unknown_lag_lead_value_defaults_as_text() {
    // Arrange
    let statements = [
        (
            "lag_unknown_string_default",
            "SELECT lag($1, $2, 'fallback') OVER ()",
            &[25, 23][..],
        ),
        (
            "lead_unknown_null_default",
            "SELECT lead($1, $2, NULL) OVER ()",
            &[25, 23][..],
        ),
        (
            "lag_unknown_parameter_default",
            "SELECT lag($1, $2, $3) OVER ()",
            &[25, 23, 25][..],
        ),
        (
            "lag_null_value_unknown_default",
            "SELECT lag(NULL, $1, $2) OVER ()",
            &[23, 25][..],
        ),
    ];
    let mut steps = Vec::with_capacity(statements.len() * 2);
    for (statement, sql, _) in statements {
        steps.push(Step::ParseFlush(wire::parse_frame(statement, sql)));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]));
    }

    let batches = run_script("issue-806-window-all-unknown-parameters", steps);

    // Act
    for (index, (_, _, expected_parameters)) in statements.iter().enumerate() {
        let parse = &batches[index * 2];
        let described = &batches[index * 2 + 1];
        let parse_error = parse
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload));

        // Assert
        assert_eq!(
            wire::error_code(parse),
            None,
            "Parse case {index}: {parse_error:?}"
        );
        assert_eq!(wire::error_code(described), None, "Describe case {index}");
        let parameters = described
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("window ParameterDescription");
        assert_eq!(
            wire::parse_parameter_description(&parameters.1).as_slice(),
            *expected_parameters
        );
        let description = described
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("window RowDescription");
        assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    }
}

#[test]
fn should_reject_bigint_window_offset_parameters_with_undefined_function_sqlstate() {
    // Arrange
    let statements = [
        ("lag_bigint_offset", "SELECT lag($1, $2) OVER ()"),
        ("lead_bigint_offset", "SELECT lead($1, $2) OVER ()"),
        ("nth_bigint_position", "SELECT nth_value($1, $2) OVER ()"),
    ];
    let mut steps = Vec::with_capacity(statements.len() * 2);
    for (statement, sql) in statements {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            sql,
            &[23, 20],
        )));
        steps.push(Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("issue-806-window-wrong-offset-oid", steps);

    // Assert
    for index in 0..statements.len() {
        let parse_error = wire::error_code(&batches[index * 2]);
        let describe_error = wire::error_code(&batches[index * 2 + 1]);
        assert_eq!(
            parse_error.or(describe_error).as_deref(),
            Some("42883"),
            "window offset case {index}"
        );
    }
}

#[test]
fn should_reject_incompatible_lag_default_parameter_types() {
    // Arrange
    let statement = "lag_incompatible_default";
    let batches = run_script(
        "issue-806-window-incompatible-default-oid",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                statement,
                "SELECT lag($1, $2, $3) OVER ()",
                &[23, 23, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let error = batches.iter().find_map(|frames| wire::error_code(frames));

    // Assert
    assert_eq!(error.as_deref(), Some("42883"));
}

#[test]
fn should_reject_unsupported_string_default_for_integer_value() {
    // Arrange
    let statement = "lag_integer_string_default";
    let batches = run_script(
        "issue-806-window-pending-default-coercion",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                statement,
                "SELECT lag($1, 1, 'fallback') OVER ()",
                &[23],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let error = batches.iter().find_map(|frames| wire::error_code(frames));

    // Assert
    assert_eq!(error.as_deref(), Some("0A000"));
}

#[test]
fn should_reject_unsupported_string_value_for_integer_default() {
    // Arrange
    let statement = "lag_string_value_integer_default";
    let batches = run_script(
        "issue-806-window-pending-value-coercion",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                statement,
                "SELECT lag('value', 1, $1) OVER ()",
                &[23],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let error = batches.iter().find_map(|frames| wire::error_code(frames));

    // Assert
    assert_eq!(error.as_deref(), Some("0A000"));
}

#[test]
fn should_reject_bigint_ntile_parameter_with_undefined_function_sqlstate() {
    // Arrange
    let statement = "ntile_bigint_parameter";
    let batches = run_script(
        "issue-806-window-ntile-bigint-parameter",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                statement,
                "SELECT ntile($1) OVER ()",
                &[20],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame(statement),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let error = batches.iter().find_map(|frames| wire::error_code(frames));

    // Assert
    assert_eq!(error.as_deref(), Some("42883"));
}

#[test]
fn should_reject_zero_ntile_bucket_count_with_invalid_parameter_sqlstate() {
    // Arrange
    let sql = "SELECT id, ntile(0) OVER (ORDER BY n) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-ntile-zero", sql);

    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22023"));
}

#[test]
fn should_return_null_for_null_ntile_bucket_count() {
    // Arrange
    let sql = "SELECT id, ntile(NULL) OVER (ORDER BY n) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-ntile-null", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 23],
        &[
            vec![Some("1".to_owned()), None],
            vec![Some("2".to_owned()), None],
            vec![Some("3".to_owned()), None],
            vec![Some("4".to_owned()), None],
        ],
    );
}

#[test]
fn should_reject_bigint_ntile_argument_with_undefined_function_sqlstate() {
    // Arrange
    let sql = "SELECT id, ntile(2147483648) OVER (ORDER BY n) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-ntile-bigint", sql);

    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("42883"));
}

#[test]
fn should_apply_lag_offset_defaults_as_int4_results() {
    // Arrange
    let sql = "SELECT id, lag(n, 2, -1) OVER (PARTITION BY grp ORDER BY id) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-lag-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 23],
        &[
            vec![Some("1".to_owned()), Some("-1".to_owned())],
            vec![Some("2".to_owned()), Some("-1".to_owned())],
            vec![Some("3".to_owned()), Some("10".to_owned())],
            vec![Some("4".to_owned()), Some("-1".to_owned())],
        ],
    );
}

#[test]
fn should_apply_lead_offset_defaults_as_int4_results() {
    // Arrange
    let sql = "SELECT id, lead(n, 2, -1) OVER (PARTITION BY grp ORDER BY id) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-lead-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 23],
        &[
            vec![Some("1".to_owned()), Some("30".to_owned())],
            vec![Some("2".to_owned()), Some("-1".to_owned())],
            vec![Some("3".to_owned()), Some("-1".to_owned())],
            vec![Some("4".to_owned()), Some("-1".to_owned())],
        ],
    );
}

#[test]
fn should_report_nth_value_frame_result_with_int4_identity() {
    // Arrange
    let sql = "SELECT id, nth_value(n, 2) OVER (PARTITION BY grp ORDER BY id \
               ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) \
               FROM issue_806_window_wire ORDER BY id";

    // Act
    let frames = run_window_query("issue-806-window-nth-value-pgwire", sql);

    // Assert
    assert_window_rows(
        &frames,
        &[23, 23],
        &[
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("10".to_owned())],
            vec![Some("3".to_owned()), Some("10".to_owned())],
            vec![Some("4".to_owned()), None],
        ],
    );
}

#[test]
fn should_reject_nonpositive_nth_value_with_sqlstate_22016() {
    // Arrange
    let sql = "SELECT nth_value(n, 0) OVER (PARTITION BY grp ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) FROM issue_806_window_wire";

    // Act
    let frames = run_window_query("issue-806-window-nth-value-zero", sql);

    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22016"));
}
