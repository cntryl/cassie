//! VECTOR[] is excluded on wire; BOOL[] keeps its independently pinned OID.

use super::support_pgwire as wire;
use super::support_type_metadata_contract as fixture;

const BOOL_ARRAY_BINARY: &[u8] = &[
    0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 16, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, 1, 0, 255, 255, 255, 255,
    0, 0, 0, 1, 1,
];

const UNSUPPORTED_QUERIES: [&str; 5] = [
    "SELECT vectors FROM metadata_vector_empty",
    "SELECT vectors FROM metadata_vector_rows WHERE k = 1",
    "SELECT vectors FROM metadata_vector_rows WHERE k = 2",
    "SELECT vectors FROM metadata_vector_rows LIMIT 0",
    "SELECT CAST(NULL AS VECTOR(7016)[]) AS vectors",
];

#[test]
fn should_reject_vector_array_simple_queries_before_metadata() {
    // Arrange
    let cycles = UNSUPPORTED_QUERIES
        .map(|sql| vec![wire::simple_query_frame(sql)])
        .to_vec();

    // Act
    let batches = fixture::run_cycles(
        "type-vector-array-simple",
        &fixture::vector_array_setup(),
        cycles,
    );

    // Assert
    for frames in batches {
        fixture::assert_unsupported_before_metadata(&frames);
    }
}

#[test]
fn should_reject_vector_array_returning_before_mutation() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle(
            "INSERT INTO metadata_vector_rows (k, vectors) VALUES (3, NULL) RETURNING vectors",
            None,
            0,
            false,
        ),
        fixture::execute_cycle(
            "SELECT COUNT(*) FROM metadata_vector_rows WHERE k = 3",
            None,
            0,
            false,
        ),
    ];

    // Act
    let batches = fixture::run_cycles(
        "type-vector-array-prewrite",
        &fixture::vector_array_setup(),
        cycles,
    );

    // Assert
    fixture::assert_unsupported_before_metadata(&batches[0]);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("0".to_string())]]
    );
}

#[test]
fn should_reject_vector_array_statement_describe_before_metadata() {
    // Arrange
    // These independent constants expose the historical collision: BOOL[]34016
    // and VECTOR(7016)[]34016. No production type_oid function is the oracle.
    let cycles = UNSUPPORTED_QUERIES
        .map(|sql| {
            vec![
                wire::parse_frame("", sql),
                wire::describe_statement_frame(""),
                wire::sync_frame(),
            ]
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles(
        "type-vector-array-statement",
        &fixture::vector_array_setup(),
        cycles,
    );

    // Assert
    assert_eq!(batches.len(), 5);
    for frames in batches {
        fixture::assert_unsupported_before_metadata(&frames);
    }
}

#[test]
fn should_reject_vector_array_portal_describe_before_metadata() {
    // Arrange
    // Default text leaves Bind's internal explicit-format Describe unused.
    let cycles = UNSUPPORTED_QUERIES
        .map(|sql| {
            vec![
                wire::parse_frame("", sql),
                wire::bind_frame_with_formats("", "", &[], &[], &[]),
                wire::describe_portal_frame(""),
                wire::sync_frame(),
            ]
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles(
        "type-vector-array-portal",
        &fixture::vector_array_setup(),
        cycles,
    );

    // Assert
    assert_eq!(batches.len(), 5);
    for frames in batches {
        fixture::assert_unsupported_before_metadata(&frames);
    }
}

#[test]
fn should_reject_vector_array_execution_before_metadata() {
    // Arrange
    let cycles = UNSUPPORTED_QUERIES
        .map(|sql| fixture::execute_cycle(sql, None, 0, false))
        .to_vec();

    // Act
    let batches = fixture::run_cycles(
        "type-vector-array-execute",
        &fixture::vector_array_setup(),
        cycles,
    );

    // Assert
    assert_eq!(batches.len(), 5);
    for frames in batches {
        fixture::assert_unsupported_before_metadata(&frames);
    }
}

#[test]
fn should_reject_vector_array_binary_bind_before_metadata() {
    // Arrange
    // Explicit binary formats trigger the production Bind Describe boundary.
    let cycles = UNSUPPORTED_QUERIES
        .map(|sql| {
            vec![
                wire::parse_frame("", sql),
                wire::bind_frame_with_formats("", "", &[], &[], &[1]),
                wire::sync_frame(),
            ]
        })
        .to_vec();

    // Act
    let batches = fixture::run_cycles(
        "type-vector-array-binary-bind",
        &fixture::vector_array_setup(),
        cycles,
    );

    // Assert
    assert_eq!(batches.len(), 5);
    for frames in batches {
        fixture::assert_unsupported_before_metadata(&frames);
    }
}

#[test]
fn should_preserve_boolean_array_wire_identity_after_vector_array_rejection() {
    // Arrange
    let mut cycles = vec![fixture::execute_cycle(
        "SELECT vectors FROM metadata_vector_empty",
        None,
        0,
        false,
    )];
    cycles.extend([0, 1].map(|format| {
        fixture::execute_cycle(
            "SELECT flags FROM metadata_boolean_rows",
            None,
            format,
            true,
        )
    }));

    // Act
    let batches = fixture::run_cycles(
        "type-array-collision-control",
        &fixture::vector_array_setup(),
        cycles,
    );

    // Assert
    fixture::assert_unsupported_before_metadata(&batches[0]);
    for (format, frames) in [0, 1].into_iter().zip(&batches[1..]) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (34_016, -1, -1), format, true);
        let expected = if format == 0 {
            fixture::one_field_row(b"[false,null,true]")
        } else {
            fixture::one_field_row(BOOL_ARRAY_BINARY)
        };
        assert_eq!(fixture::data_row_payloads(frames), vec![expected]);
    }
}
