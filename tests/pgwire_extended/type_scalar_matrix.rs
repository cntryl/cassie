//! Missing finite scalar descriptor/codec combinations for selected #752.

use super::support_type_codec_matrix as matrix;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_scalar_codec_identity_across_wire_formats() {
    // Arrange
    let setup = matrix::setup_queries("scalar_values", &matrix::SCALAR_CASES);
    let (cycles, expected) = matrix::populated_cycles("scalar_values", &matrix::SCALAR_CASES);

    // Act
    let batches = fixture::run_cycles("type-scalar-codecs", &matrix::setup_refs(&setup), cycles);

    // Assert
    assert_eq!(matrix::SCALAR_CASES.len(), 15);
    assert_eq!(batches.len(), 90);
    matrix::assert_batches(&batches, &expected);
}

#[test]
fn should_preserve_scalar_descriptors_for_empty_outputs() {
    // Arrange
    let setup = matrix::setup_queries("scalar_empty", &matrix::SCALAR_CASES);
    let (cycles, expected) =
        matrix::descriptor_cycles("scalar_empty", &matrix::SCALAR_CASES, false);

    // Act
    let batches = fixture::run_cycles("type-scalar-empty", &matrix::setup_refs(&setup), cycles);

    // Assert
    assert_eq!(batches.len(), 30);
    matrix::assert_batches(&batches, &expected);
}

#[test]
fn should_preserve_scalar_descriptors_for_null_outputs() {
    // Arrange
    let (cycles, expected) = matrix::descriptor_cycles("unused", &matrix::SCALAR_CASES, true);

    // Act
    let batches = fixture::run_cycles("type-scalar-null", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 30);
    matrix::assert_batches(&batches, &expected);
}

#[test]
fn should_preserve_oid_identity_across_pgwire_formats() {
    // Arrange
    let inputs = [(0, &b"23"[..]), (1, &[0, 0, 0, 23][..])];
    let cycles = inputs
        .into_iter()
        .flat_map(|(input_format, payload)| {
            [0, 1].map(|result_format| {
                fixture::execute_cycle(
                    "SELECT $1 AS v",
                    Some((26, input_format, Some(payload))),
                    result_format,
                    true,
                )
            })
        })
        .collect();

    // Act
    let batches = fixture::run_cycles("type-oid-text-binary-roundtrip", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for (input_index, pair) in batches.as_chunks::<2>().0.iter().enumerate() {
        for (result_format, frames) in [0, 1].into_iter().zip(pair) {
            fixture::assert_success(frames);
            fixture::assert_single_column_descriptors(frames, (26, 4, -1), result_format, true);
            assert_eq!(
                fixture::data_row_payloads(frames),
                vec![fixture::one_field_row(if result_format == 0 {
                    &b"23"[..]
                } else {
                    &[0, 0, 0, 23][..]
                })],
                "input format index {input_index}, result format {result_format}"
            );
        }
    }
}
