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
