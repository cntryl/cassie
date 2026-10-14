//! Named-argument regex function binding through the PostgreSQL wire protocol.

use crate::support_parameter_occurrence_wire::{Step, run_script};
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
