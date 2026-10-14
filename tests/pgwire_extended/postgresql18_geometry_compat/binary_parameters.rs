//! PostgreSQL geometry binary Bind input contracts tracked by #907.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

fn run_binary_geometry_input(
    label: &str,
    statement: &str,
    parameter_oid: i32,
    payload: &[u8],
) -> Vec<Vec<(u8, Vec<u8>)>> {
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            statement,
            "SELECT $1 AS value",
            &[parameter_oid],
        )),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                &format!("{statement}_portal"),
                statement,
                &[1],
                &[Some(payload)],
                &[0],
            ),
            wire::describe_portal_frame(&format!("{statement}_portal")),
            wire::execute_frame(&format!("{statement}_portal")),
            wire::sync_frame(),
        ]),
    ];

    run_script(label, steps)
}

#[test]
fn should_decode_binary_point_bind_parameter_with_postgresql_identity() {
    // Arrange
    let point_binary = [
        0x3f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];

    // Act
    let batches = run_binary_geometry_input(
        "issue-907-point-binary-input",
        "point_binary_input",
        600,
        &point_binary,
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[1];
    assert_eq!(wire::error_code(described), None);
    let parameter_description = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("point ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![600]
    );
    let column = &wire::parse_row_description(
        &described
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("point statement RowDescription")
            .1,
    )[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (600, 16, -1, 0)
    );
    let result = &batches[2];
    assert_eq!(wire::error_code(result), None);
    let column = &wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("point portal RowDescription")
            .1,
    )[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (600, 16, -1)
    );
    assert_eq!(column.format_code, 0);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("(1,2)".to_owned())]]
    );
}

#[test]
fn should_decode_binary_box_bind_parameter_with_postgresql_identity() {
    // Arrange
    let box_binary = [
        0x40, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x3f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];

    // Act
    let batches = run_binary_geometry_input(
        "issue-907-box-binary-input",
        "box_binary_input",
        603,
        &box_binary,
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[1];
    assert_eq!(wire::error_code(described), None);
    let parameter_description = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("box ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![603]
    );
    let column = &wire::parse_row_description(
        &described
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("box statement RowDescription")
            .1,
    )[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (603, 32, -1, 0)
    );
    let result = &batches[2];
    assert_eq!(wire::error_code(result), None);
    let column = &wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("box portal RowDescription")
            .1,
    )[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (603, 32, -1)
    );
    assert_eq!(column.format_code, 0);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("(3,4),(1,2)".to_owned())]]
    );
}

#[test]
fn should_decode_binary_path_bind_parameter_with_postgresql_identity() {
    // Arrange
    let path_binary = [
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x3f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x08, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x40, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    // Act
    let batches = run_binary_geometry_input(
        "issue-907-path-binary-input",
        "path_binary_input",
        602,
        &path_binary,
    );

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let described = &batches[1];
    assert_eq!(wire::error_code(described), None);
    let parameter_description = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("path ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![602]
    );
    let column = &wire::parse_row_description(
        &described
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("path statement RowDescription")
            .1,
    )[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (602, -1, -1, 0)
    );
    let result = &batches[2];
    assert_eq!(wire::error_code(result), None);
    let column = &wire::parse_row_description(
        &result
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("path portal RowDescription")
            .1,
    )[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (602, -1, -1)
    );
    assert_eq!(column.format_code, 0);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("[(1,2),(3,4)]".to_owned())]]
    );
}
