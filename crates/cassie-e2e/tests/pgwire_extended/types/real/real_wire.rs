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

#[test]
fn should_preserve_nonfinite_real_parameter_bits_in_binary_results() {
    // Arrange
    let sql = "SELECT CAST($1 AS real) AS v";
    let inputs = [
        f32::INFINITY.to_bits(),
        f32::NEG_INFINITY.to_bits(),
        0x7FC0_0001,
        0xFFC0_0002,
        0x7F80_0001,
        0x7FC0_0001,
        0.0_f32.to_bits(),
        (-0.0_f32).to_bits(),
    ];
    let cycles = inputs
        .iter()
        .map(|value| {
            fixture::execute_cycle(sql, Some((700, 1, Some(&value.to_be_bytes()))), 1, true)
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-real-nonfinite-binary", &[], cycles);

    // Assert
    assert_eq!(batches.len(), inputs.len());
    for (frames, value) in batches.iter().zip(inputs) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (700, 4, -1), 1, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&value.to_be_bytes())]
        );
    }
}
