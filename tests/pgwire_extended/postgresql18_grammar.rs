//! PostgreSQL 18 parser, collation, and wire contracts tracked by #796.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_describe_postgres_cast_parameter_oid_through_extended_protocol() {
    // Arrange
    let batches = run_script(
        "issue-796-cast-parameter-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "cast_parameter",
                "SELECT $1::DOUBLE PRECISION AS value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("cast_parameter"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(described), None);
    let parameter_types = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![701]
    );
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 701);
}

#[test]
fn should_report_postgres_collation_conflicts_with_sqlstate_42p21() {
    // Arrange
    let batches = run_script(
        "issue-796-collation-conflict-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ('a' COLLATE \"C\") < ('b' COLLATE \"POSIX\")",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42P21"));
}

#[test]
fn should_expose_the_builtin_postgres_collation_catalog_rows() {
    // Arrange
    let batches = run_script(
        "issue-796-pg-collation-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT collname, collprovider, collisdeterministic FROM pg_collation WHERE collname = 'C'",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        wire::row_description_names(result),
        vec!["collname", "collprovider", "collisdeterministic"]
    );
    assert_eq!(wire::data_rows(result).len(), 1);
    assert_eq!(wire::data_rows(result)[0][0].as_deref(), Some("C"));
}
