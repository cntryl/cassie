//! All supported families retain identity when Bind supplies field length−1.

use super::support_type_codec_matrix as matrix;
use super::support_type_declared_null_matrix as null_matrix;
use super::support_type_metadata_contract as fixture;

#[test]
fn should_preserve_declared_scalar_nulls_across_wire_formats() {
    // Arrange
    let setup = matrix::setup_queries("scalar_bind_null", &matrix::SCALAR_CASES);
    let cycles = null_matrix::cycles("scalar_bind_null", &matrix::SCALAR_CASES);

    // Act
    let batches = fixture::run_cycles("type-scalar-bind-null", &matrix::setup_refs(&setup), cycles);

    // Assert
    assert_eq!(batches.len(), 90);
    null_matrix::assert_batches(&batches, &matrix::SCALAR_CASES);
}

#[test]
fn should_preserve_declared_scalar_array_nulls_across_wire_formats() {
    // Arrange
    let setup = matrix::setup_queries("array_bind_null", &matrix::ARRAY_CASES);
    let cycles = null_matrix::cycles("array_bind_null", &matrix::ARRAY_CASES);

    // Act
    let batches = fixture::run_cycles("type-array-bind-null", &matrix::setup_refs(&setup), cycles);

    // Assert
    assert_eq!(batches.len(), 84);
    null_matrix::assert_batches(&batches, &matrix::ARRAY_CASES);
}
