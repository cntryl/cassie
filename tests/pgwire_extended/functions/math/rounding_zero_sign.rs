use super::{run_script, wire, Step};

#[test]
fn should_preserve_float8_zero_sign_for_round_trunc_floor_ceil_abs() {
    // Arrange
    use crate::support_type_metadata_contract as fixture;

    let statement = "float8_signed_zero_math";
    let portal = "float8_signed_zero_math_binary";
    let sql = "SELECT ROUND(CAST(-0.1 AS FLOAT8)), TRUNC(CAST(-0.1 AS FLOAT8)), CEIL(CAST(-0.1 AS FLOAT8)), FLOOR(CAST(-0.0 AS FLOAT8)), ABS(CAST(-0.0 AS FLOAT8))";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(portal, statement, &[], &[], &[1]),
            wire::describe_portal_frame(portal),
            wire::execute_frame(portal),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-float8-signed-zero-math", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let description = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("signed-zero math RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.format_code))
            .collect::<Vec<_>>(),
        vec![(701, 8, 1); 5]
    );
    let mut expected = 5_i16.to_be_bytes().to_vec();
    for bits in [
        [0x80, 0, 0, 0, 0, 0, 0, 0],
        [0x80, 0, 0, 0, 0, 0, 0, 0],
        [0x80, 0, 0, 0, 0, 0, 0, 0],
        [0x80, 0, 0, 0, 0, 0, 0, 0],
        [0; 8],
    ] {
        expected.extend_from_slice(&8_i32.to_be_bytes());
        expected.extend_from_slice(&bits);
    }
    assert_eq!(fixture::data_row_payloads(&batches[1]), vec![expected]);
}
