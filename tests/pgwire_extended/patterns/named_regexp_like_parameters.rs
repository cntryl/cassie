//! Named-argument regex function binding through the PostgreSQL wire protocol.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_bind_named_regexp_like_arguments_over_pgwire() {
    // Arrange
    let sql = "SELECT regexp_like(string => $1, pattern => $2, flags => 'i') AS matched";
    let batches = run_script(
        "issue-799-regexp-like-named-parameters-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "named_regexp_like",
                sql,
                &[25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("named_regexp_like"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "named_regexp_like_portal",
                    "named_regexp_like",
                    &[0],
                    &[Some(b"Alpha"), Some(b"^alpha$")],
                    &[1],
                ),
                wire::describe_portal_frame("named_regexp_like_portal"),
                wire::execute_frame("named_regexp_like_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("named regexp_like ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25]
    );
    let statement_column = wire::parse_row_description(
        &described
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("named regexp_like statement RowDescription")
            .1,
    )
    .remove(0);
    assert_eq!(statement_column.type_oid, 16);
    assert_eq!(statement_column.type_size, 1);
    let portal_column = wire::parse_row_description(
        &executed
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("named regexp_like portal RowDescription")
            .1,
    )
    .remove(0);
    assert_eq!(portal_column.type_oid, 16);
    assert_eq!(portal_column.type_size, 1);
    assert_eq!(portal_column.format_code, 1);
    assert_eq!(
        executed
            .iter()
            .find(|frame| frame.0 == b'D')
            .expect("binary Boolean DataRow")
            .1,
        vec![0, 1, 0, 0, 0, 1, 1]
    );
}

#[test]
fn should_accept_parameterized_named_regexp_count_syntax_over_pgwire() {
    // Arrange
    let sql =
        "SELECT regexp_count(string => $1, pattern => $2, start => $3, flags => $4) AS matches";
    let batches = run_script(
        "issue-799-regexp-count-named-parameters-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "named_regexp_count",
                sql,
                &[0, 0, 0, 0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("named_regexp_count"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "named_regexp_count_portal",
                    "named_regexp_count",
                    &["ABCABCAXYaxy", "A.", "1", "i"],
                ),
                wire::describe_portal_frame("named_regexp_count_portal"),
                wire::execute_frame("named_regexp_count_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(
        wire::error_code(parsed),
        None,
        "named regexp_count Parse errors: {:?}",
        parsed
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(described),
        None,
        "named regexp_count Describe errors: {:?}",
        described
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(executed),
        None,
        "named regexp_count Bind/Execute errors: {:?}",
        executed
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("named regexp_count ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25, 23, 25]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("named regexp_count RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        20
    );
    assert_eq!(wire::data_rows(executed), vec![vec![Some("4".to_owned())]]);
}

#[test]
fn should_execute_positional_regexp_count_call_over_pgwire() {
    // Arrange
    let sql = "SELECT regexp_count($1, $2, $3, $4) AS matches";
    let batches = run_script(
        "issue-799-regexp-count-positional-parameters-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "positional_regexp_count",
                sql,
                &[25, 25, 23, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("positional_regexp_count"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "positional_regexp_count_portal",
                    "positional_regexp_count",
                    &["ABCABCAXYaxy", "A.", "1", "i"],
                ),
                wire::describe_portal_frame("positional_regexp_count_portal"),
                wire::execute_frame("positional_regexp_count_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(
        wire::error_code(parsed),
        None,
        "positional regexp_count Parse errors: {:?}",
        parsed
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(described),
        None,
        "positional regexp_count Describe errors: {:?}",
        described
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::error_code(executed),
        None,
        "positional regexp_count Bind/Execute errors: {:?}",
        executed
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("positional regexp_count ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![25, 25, 23, 25]
    );
    let result_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("positional regexp_count RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_description.1)[0].type_oid,
        20
    );
    assert_eq!(wire::data_rows(executed), vec![vec![Some("4".to_owned())]]);
}
