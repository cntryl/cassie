//! `bpchar` type names, casts and typmods over the existing character identity.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_bind_bpchar_casts_using_native_type_metadata() {
    // Arrange
    let statement = "bpchar_casts";
    let portal = "bpchar_portal";
    let sql = "SELECT $1::pg_catalog.bpchar AS unbounded, \
        $2::pg_catalog.bpchar(3) AS bounded";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(statement, sql, &[0, 0])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame(portal, statement, &["x", "y"]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-796-bpchar-casts", steps);
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None, "Parse must succeed");
    assert_eq!(wire::error_code(described), None, "Describe must succeed");
    assert_eq!(
        wire::error_code(executed),
        None,
        "Bind/Execute must succeed"
    );
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("bpchar ParameterDescription")
                .1
        ),
        vec![1_042, 1_042]
    );

    let expected_columns = vec![
        ("unbounded", 1_042, -1_i16, -1_i32, 0_i16),
        ("bounded", 1_042, -1_i16, 7_i32, 0_i16),
    ];
    for frames in [described, executed] {
        let description = frames
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("bpchar RowDescription");
        assert_eq!(
            wire::parse_row_description(&description.1)
                .iter()
                .map(|column| (
                    column.name.as_str(),
                    column.type_oid,
                    column.type_size,
                    column.type_mod,
                    column.format_code,
                ))
                .collect::<Vec<_>>(),
            expected_columns
        );
    }
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![Some("x".to_owned()), Some("y  ".to_owned())]]
    );
}
