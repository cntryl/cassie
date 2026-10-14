use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

#[test]
fn should_infer_point_parameter_for_slope_in_float_expression() {
    // Arrange
    let statement = "geometry_slope_unknown_parameter";
    let portal = "geometry_slope_unknown_parameter_portal";
    let sql = "SELECT slope($1, point '(4,6)') AS base_slope, \
                      slope($1, point '(4,6)') + 0.5 AS offset_slope";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::bind_frame_with_formats(portal, statement, &[0], &[Some(b"(1,2)")], &[0]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-907-slope-unknown-point-parameter", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));

    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let parameter_description = result
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("slope ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![600],
        "PostgreSQL 18.6 infers the untyped slope argument as point"
    );

    let descriptions = result
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, payload)| wire::parse_row_description(payload))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2);
    for description in descriptions {
        assert_eq!(
            description
                .iter()
                .map(|column| (
                    column.type_oid,
                    column.type_size,
                    column.type_mod,
                    column.format_code,
                ))
                .collect::<Vec<_>>(),
            vec![(701, 8, -1, 0), (701, 8, -1, 0)]
        );
    }
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("1.3333333333333333".to_owned()),
            Some("1.8333333333333333".to_owned()),
        ]]
    );
}
