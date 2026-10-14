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

#[test]
fn should_describe_pg_typeof_with_postgresql_regtype_identity() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT pg_typeof(1)", None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-pg-typeof-regtype-rowdescription", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (2_206, 4, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(frames),
        vec![fixture::one_field_row(b"integer")]
    );
}

#[test]
fn should_roundtrip_regtype_over_wire_formats() {
    // Arrange
    let formats = [0, 1];
    let cycles = formats
        .map(|format| fixture::execute_cycle("SELECT 'integer'::regtype", None, format, true))
        .to_vec();

    // Act
    let batches = fixture::run_cycles("type-regtype-text-binary", &[], cycles);

    // Assert
    assert_eq!(batches.len(), formats.len());
    for (format, frames) in formats.into_iter().zip(batches) {
        fixture::assert_success(&frames);
        fixture::assert_single_column_descriptors(&frames, (2_206, 4, -1), format, true);
        let value = if format == 0 {
            b"integer".as_slice()
        } else {
            &[0, 0, 0, 23]
        };
        assert_eq!(
            fixture::data_row_payloads(&frames),
            vec![fixture::one_field_row(value)],
            "result format {format}"
        );
    }
}
