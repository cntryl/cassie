//! Native `real` (`float4`) descriptors and text/binary wire framing.

use super::support_type_metadata_contract as fixture;

#[test]
fn should_frame_real_parameters_in_each_wire_format() {
    // Arrange
    let sql = "SELECT CAST($1 AS real) AS v";
    let cycles = vec![
        fixture::execute_cycle(sql, Some((700, 0, Some(b"2.5"))), 0, true),
        fixture::execute_cycle(sql, Some((700, 1, Some(&2.5_f32.to_be_bytes()))), 1, true),
    ];

    // Act
    let batches = fixture::run_cycles("type-real-wire-formats", &[], cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (700, 4, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![fixture::one_field_row(b"2.5")]
    );
    fixture::assert_success(&batches[1]);
    fixture::assert_single_column_descriptors(&batches[1], (700, 4, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row(&2.5_f32.to_be_bytes())]
    );
}
