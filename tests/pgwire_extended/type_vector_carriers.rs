//! Exact current vector carrier text spellings and binary identity.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

const VECTOR_BINARY: &[u8] = &[0, 2, 0, 0, 63, 192, 0, 0, 192, 0, 0, 0];
const VECTOR_TEXT: &[u8] = b"[1.5,-2.0]";
const VECTOR_NATIVE_TEXT: &[u8] = b"[1.5,-2]";
const PARAMETER_QUERIES: [&str; 3] = [
    "SELECT $1 AS v",
    "SELECT v FROM (SELECT $1 AS v) AS d",
    "WITH c AS (SELECT $1 AS v) SELECT v FROM c",
];

#[test]
fn should_encode_declared_text_vector_parameters_in_binary_results() {
    // Arrange
    let mut cycles = Vec::new();
    for sql in PARAMETER_QUERIES {
        for describe in [false, true] {
            cycles.push(fixture::execute_cycle(
                sql,
                Some((100_002, 0, Some(VECTOR_TEXT))),
                1,
                describe,
            ));
        }
    }

    // Act
    let batches = fixture::run_cycles("type-vector-text-binary-output", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 6);
    for (index, frames) in batches.iter().enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (100_002, -1, -1), 1, index % 2 == 1);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(VECTOR_BINARY)],
        );
    }
}

#[test]
fn should_preserve_declared_vector_parameter_text_spellings() {
    // Arrange
    let mut cycles = Vec::new();
    let mut expected = Vec::new();
    for sql in PARAMETER_QUERIES {
        for (input_format, input, output) in [
            (0, VECTOR_TEXT, VECTOR_TEXT),
            (1, VECTOR_BINARY, VECTOR_NATIVE_TEXT),
        ] {
            for describe in [false, true] {
                cycles.push(fixture::execute_cycle(
                    sql,
                    Some((100_002, input_format, Some(input))),
                    0,
                    describe,
                ));
                expected.push((describe, output));
            }
        }
    }

    // Act
    let batches = fixture::run_cycles("type-vector-parameter-text-spellings", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 12);
    for (frames, (describe, output)) in batches.iter().zip(expected) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (100_002, -1, -1), 0, describe);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(output)],
        );
    }
}

#[test]
fn should_preserve_native_vector_parameters_in_binary_results() {
    // Arrange
    let mut cycles = Vec::new();
    for sql in PARAMETER_QUERIES {
        for describe in [false, true] {
            cycles.push(fixture::execute_cycle(
                sql,
                Some((100_002, 1, Some(VECTOR_BINARY))),
                1,
                describe,
            ));
        }
    }

    // Act
    let batches = fixture::run_cycles("type-vector-native-binary-output", &[], cycles);

    // Assert
    assert_eq!(batches.len(), 6);
    for (index, frames) in batches.iter().enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (100_002, -1, -1), 1, index % 2 == 1);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(VECTOR_BINARY)],
        );
    }
}

#[test]
fn should_reject_nonnullable_vector_nulls_before_document_publication() {
    // Arrange
    let setup = [(
        "CREATE TABLE type_vector_not_null (k INT, v VECTOR(2) NOT NULL)",
        Vec::new(),
    )];
    let mut cycles = Vec::new();
    for input_format in [0, 1] {
        for result_format in [0, 1] {
            cycles.push(fixture::execute_cycle(
                "INSERT INTO type_vector_not_null (k, v) VALUES (1, $1) RETURNING v",
                Some((100_002, input_format, None)),
                result_format,
                true,
            ));
        }
    }
    cycles.push(fixture::execute_cycle(
        "SELECT COUNT(*) AS n FROM type_vector_not_null",
        None,
        0,
        false,
    ));

    // Act
    let batches = fixture::run_cycles("type-vector-not-null-write-boundary", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 5);
    for (index, frames) in batches[..4].iter().enumerate() {
        assert_eq!(wire::error_code(frames).as_deref(), Some("23502"));
        fixture::assert_single_column_descriptors(
            frames,
            (100_002, -1, -1),
            i16::try_from(index % 2).expect("result format"),
            true,
        );
        assert_eq!(fixture::data_row_payloads(frames), Vec::<Vec<u8>>::new());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
    fixture::assert_success(&batches[4]);
    fixture::assert_single_column_descriptors(&batches[4], (20, 8, -1), 0, false);
    assert_eq!(
        fixture::data_row_payloads(&batches[4]),
        vec![fixture::one_field_row(b"0")]
    );
}

#[test]
fn should_preserve_nonnull_vector_width_validation_before_publication() {
    // Arrange
    let setup = [(
        "CREATE TABLE type_vector_width (k INT, v VECTOR(2) NOT NULL)",
        Vec::new(),
    )];
    let mut cycles = Vec::new();
    for (key, input_format, input) in [(1, 0, VECTOR_TEXT), (2, 1, VECTOR_BINARY)] {
        let sql = format!("INSERT INTO type_vector_width (k, v) VALUES ({key}, $1) RETURNING v");
        cycles.push(fixture::execute_cycle(
            &sql,
            Some((100_002, input_format, Some(input))),
            1,
            true,
        ));
    }
    cycles.push(fixture::execute_cycle(
        "INSERT INTO type_vector_width (k, v) VALUES (3, $1) RETURNING v",
        Some((100_002, 0, Some(b"[1]"))),
        0,
        true,
    ));
    cycles.push(fixture::execute_cycle(
        "SELECT COUNT(*) AS n FROM type_vector_width",
        None,
        0,
        false,
    ));

    // Act
    let batches = fixture::run_cycles("type-vector-nonnull-width-boundary", &setup, cycles);

    // Assert
    assert_eq!(batches.len(), 4);
    for frames in &batches[..2] {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (100_002, -1, -1), 1, true);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(VECTOR_BINARY)]
        );
    }
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("22000"));
    fixture::assert_single_column_descriptors(&batches[2], (100_002, -1, -1), 0, true);
    assert_eq!(
        fixture::data_row_payloads(&batches[2]),
        Vec::<Vec<u8>>::new()
    );
    assert_eq!(batches[2].iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    assert_eq!(
        batches[2].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    fixture::assert_success(&batches[3]);
    fixture::assert_single_column_descriptors(&batches[3], (20, 8, -1), 0, false);
    assert_eq!(
        fixture::data_row_payloads(&batches[3]),
        vec![fixture::one_field_row(b"2")]
    );
}

#[test]
fn should_preserve_finite_vector_boundaries_through_text_parameter_writes() {
    // Arrange
    let binary = [0, 2, 0, 0, 127, 127, 255, 255, 255, 127, 255, 255];
    let text = b"[3.4028234663852886e38,-3.4028234663852886e38]";
    let setup = [
        ("CREATE TABLE native_boundary_vectors (v VECTOR(2))", vec![]),
        ("CREATE TABLE text_boundary_vectors (v VECTOR(2))", vec![]),
    ];
    let cycles = vec![
        fixture::execute_cycle(
            "INSERT INTO native_boundary_vectors (v) VALUES ($1) RETURNING v",
            Some((100_002, 1, Some(&binary))),
            1,
            false,
        ),
        fixture::execute_cycle("SELECT v FROM native_boundary_vectors", None, 1, false),
        fixture::execute_cycle(
            "INSERT INTO text_boundary_vectors (v) VALUES ($1) RETURNING v",
            Some((100_002, 0, Some(text))),
            1,
            false,
        ),
        fixture::execute_cycle("SELECT v FROM text_boundary_vectors", None, 1, false),
    ];

    // Act
    let frames = fixture::run_cycles("finite-vector-write-boundaries", &setup, cycles);

    // Assert
    assert_eq!(frames.len(), 4);
    for (index, cycle) in frames.iter().enumerate() {
        fixture::assert_success(cycle);
        fixture::assert_single_column_descriptors(cycle, (100_002, -1, -1), 1, false);
        assert_eq!(
            fixture::data_row_payloads(cycle),
            vec![fixture::one_field_row(&binary)],
            "boundary carrier cycle {index}"
        );
    }
}

#[test]
fn should_preserve_vector_text_write_error_precedence_before_publication() {
    // Arrange
    let setup = [(
        "CREATE TABLE vector_text_rejection_preservation (v VECTOR(2))",
        Vec::new(),
    )];
    let inputs = [
        ("[1e39]", false),
        ("[1e39,0,0]", false),
        ("[1e39,0]", true),
        ("[1e400,0]", false),
        ("[null,0]", false),
        ("[\"1\",0]", false),
        ("[1,0] trailing", false),
        ("[NaN,0]", false),
    ];
    let mut cycles = inputs
        .iter()
        .map(|(input, _)| {
            fixture::execute_cycle(
                "INSERT INTO vector_text_rejection_preservation (v) VALUES ($1) RETURNING v",
                Some((100_002, 0, Some(input.as_bytes()))),
                0,
                false,
            )
        })
        .collect::<Vec<_>>();
    cycles.push(fixture::execute_cycle(
        "SELECT COUNT(*) AS n FROM vector_text_rejection_preservation",
        None,
        0,
        false,
    ));

    // Act
    let frames = fixture::run_cycles("vector-text-rejection-precedence", &setup, cycles);

    // Assert
    assert_eq!(frames.len(), inputs.len() + 1);
    for ((input, range), frames) in inputs.into_iter().zip(&frames) {
        assert_eq!(
            wire::error_code(frames).as_deref(),
            Some("22000"),
            "input {input}"
        );
        let errors = frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        let message = wire::parse_error_fields(&errors[0].1)
            .into_iter()
            .find_map(|(key, value)| (key == 'M').then_some(value))
            .expect("error message");
        if range {
            assert_eq!(
                message, "field 'v' vector element is outside f32 range",
                "input {input}"
            );
        } else {
            assert_eq!(
                message, "invalid vector: field 'v' expects vector(2)",
                "input {input}"
            );
        }
        assert_eq!(
            frames
                .iter()
                .filter(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
                .count(),
            0
        );
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
    }
    let count = frames.last().expect("count control");
    fixture::assert_success(count);
    fixture::assert_single_column_descriptors(count, (20, 8, -1), 0, false);
    assert_eq!(
        fixture::data_row_payloads(count),
        vec![fixture::one_field_row(b"0")]
    );
}

#[test]
fn should_preserve_finite_vector_query_values_across_text_parameter_fallback() {
    // Arrange
    let binary = [0, 2, 0, 0, 127, 127, 255, 255, 255, 127, 255, 255];
    let text = b"[3.4028234663852886e38,-3.4028234663852886e38]";
    let setup = [
        (
            "CREATE TABLE vector_query_boundary (v VECTOR(2))",
            Vec::new(),
        ),
        (
            "INSERT INTO vector_query_boundary (v) VALUES ($1)",
            vec![cassie::types::Value::Vector(cassie::types::Vector::new(
                vec![f32::from_bits(0x7f7f_ffff), f32::from_bits(0xff7f_ffff)],
            ))],
        ),
    ];
    let queries = [
        "SELECT vector_distance(v, $1) AS distance FROM vector_query_boundary ORDER BY distance ASC LIMIT 1",
        "SELECT vector_distance(v, $1) AS distance FROM vector_query_boundary",
    ];
    let mut cycles = Vec::new();
    for query in queries {
        for (format, bytes) in [(1, binary.as_slice()), (0, text.as_slice())] {
            cycles.push(fixture::execute_cycle(
                query,
                Some((100_002, format, Some(bytes))),
                1,
                false,
            ));
        }
    }

    // Act
    let frames = fixture::run_cycles("vector-query-boundary-text-fallback", &setup, cycles);

    // Assert
    assert_eq!(frames.len(), 4);
    for (index, frames) in frames.iter().enumerate() {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (701, 8, -1), 1, false);
        assert_eq!(
            fixture::data_row_payloads(frames),
            vec![fixture::one_field_row(&[0; 8])],
            "query carrier {index}"
        );
    }
}
