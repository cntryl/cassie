use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn binary_row(fields: &[&[u8]]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(
        &i16::try_from(fields.len())
            .expect("geometry result field count should fit i16")
            .to_be_bytes(),
    );
    for field in fields {
        payload.extend_from_slice(
            &i32::try_from(field.len())
                .expect("geometry result field length should fit i32")
                .to_be_bytes(),
        );
        payload.extend_from_slice(field);
    }
    payload
}

#[test]
fn should_preserve_point_special_float_wire_values() {
    // Arrange
    let negative_zero_positive_infinity = [
        0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7f, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    let nan_negative_infinity = [
        0x7f, 0xf8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00,
    ];
    let special_points_sql =
        "SELECT point '(-0, Infinity)' AS negative_zero, point '(NaN, -Infinity)' AS nonfinite";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(special_points_sql)]),
        Step::ParseFlush(wire::parse_frame_with_types(
            "special_point_binary_input",
            "SELECT $1::point::text AS decoded",
            &[600],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "special_point_binary_input_portal",
                "special_point_binary_input",
                &[1],
                &[Some(&negative_zero_positive_infinity)],
                &[0],
            ),
            wire::describe_portal_frame("special_point_binary_input_portal"),
            wire::execute_frame("special_point_binary_input_portal"),
            wire::sync_frame(),
        ]),
        Step::ParseFlush(wire::parse_frame(
            "special_point_binary_output",
            special_points_sql,
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "special_point_binary_output_portal",
                "special_point_binary_output",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("special_point_binary_output_portal"),
            wire::execute_frame("special_point_binary_output_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-907-point-floating-boundaries", steps);

    // Assert
    assert_eq!(batches.len(), 5);
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("(-0,Infinity)".to_owned()),
            Some("(NaN,-Infinity)".to_owned()),
        ]]
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(batches[1].first().map(|(tag, _)| *tag), Some(b'1'));
    let parameter_description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("special point ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![600]
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("(-0,Infinity)".to_owned())]]
    );
    assert_eq!(wire::error_code(&batches[3]), None);
    assert_eq!(batches[3].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[4];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("special point binary RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            ))
            .collect::<Vec<_>>(),
        vec![(600, 16, -1, 1), (600, 16, -1, 1)]
    );
    assert_eq!(
        fixture::data_row_payloads(result),
        vec![binary_row(&[
            &negative_zero_positive_infinity,
            &nan_negative_infinity,
        ])]
    );
}
