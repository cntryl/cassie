//! PostgreSQL 18 geometry wire identities tracked by #907.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_describe_a_point_cast_with_postgresql_type_identity() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT '(1,2)'::point AS value",
    )])];

    // Act
    let batches = run_script("postgresql18-point-cast-identity", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("point RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (600, 16, -1)
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("(1,2)".to_owned())]]
    );
}

#[test]
fn should_describe_builtin_geometry_values_with_postgresql_oids() {
    // Arrange
    let sql = "SELECT point '(1,2)' AS point_value, '((0,0),(1,1))'::box AS box_value, '[(1,2),(3,4)]'::lseg AS segment_value, '((0,0),(5,5),(0,5))'::polygon AS polygon_value";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("postgresql18-geometry-wire-oids", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("geometry RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![600, 603, 601, 604]
    );
}

#[test]
fn should_send_point_values_in_postgresql_binary_format() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let point_binary = [
        0x3f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "point_binary_statement",
            "SELECT '(1,2)'::point AS value",
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "point_binary_portal",
                "point_binary_statement",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("point_binary_portal"),
            wire::execute_frame("point_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-907-point-binary-output", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("point binary RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (600, 16, -1, 1)
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row(&point_binary)]
    );
}

#[test]
fn should_send_circle_values_in_postgresql_binary_format() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let circle_binary = [
        0x3f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x40, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "circle_binary_statement",
            "SELECT '<(1,2),3>'::circle AS value",
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "circle_binary_portal",
                "circle_binary_statement",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("circle_binary_portal"),
            wire::execute_frame("circle_binary_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-907-circle-binary-output", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(
        wire::error_code(result),
        None,
        "binary circle input error: {:?}",
        result
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("circle binary RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (718, 24, -1, 1)
    );
    assert_eq!(
        fixture::data_row_payloads(result),
        vec![fixture::one_field_row(&circle_binary)]
    );
}

#[test]
fn should_send_box_values_in_postgresql_binary_format() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let box_binary = [
        0x40, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x3f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(
            "box_binary_output",
            "SELECT '((1,2),(3,4))'::box AS value",
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "box_binary_output_portal",
                "box_binary_output",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("box_binary_output_portal"),
            wire::execute_frame("box_binary_output_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-907-box-binary-output", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("box binary RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.format_code),
        (603, 32, 1)
    );
    assert_eq!(
        fixture::data_row_payloads(result),
        vec![fixture::one_field_row(&box_binary)]
    );
}

#[test]
fn should_decode_binary_circle_parameters() {
    // Arrange
    let circle_binary = [
        0x3f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x40, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let sql = "SELECT $1 AS value";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "circle_binary_input",
            sql,
            &[718],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "circle_binary_input_portal",
                "circle_binary_input",
                &[1],
                &[Some(&circle_binary)],
                &[0],
            ),
            wire::describe_portal_frame("circle_binary_input_portal"),
            wire::execute_frame("circle_binary_input_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-907-circle-binary-input", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("circle binary-input RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!((column.type_oid, column.format_code), (718, 0));
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("<(1,2),3>".to_owned())]]
    );
}
