//! Residual PostgreSQL array search function behavior for #807.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

#[test]
fn should_return_all_repeated_custom_bound_subscripts() {
    // Arrange
    let input = b"[0:4]={7,NULL,7,NULL,7}";
    let cycle = fixture::execute_cycle(
        "SELECT array_position($1::INT[], 7), array_positions($1::INT[], 7)",
        Some((1_007, 0, Some(input))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-search-positions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array position RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1), (1_007, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("0".to_owned()), Some("{0,2,4}".to_owned()),]]
    );
}

#[test]
fn should_find_null_element_subscripts_for_custom_bounds() {
    // Arrange
    let input = b"[0:4]={7,NULL,7,NULL,7}";
    let cycle = fixture::execute_cycle(
        "SELECT array_position($1::INT[], NULL), array_positions($1::INT[], NULL)",
        Some((1_007, 0, Some(input))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-null-search-positions", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("array null position RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1), (1_007, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("1".to_owned()), Some("{1,3}".to_owned())]]
    );
}

#[test]
fn should_return_null_for_absent_array_position() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT array_position(ARRAY[7,NULL,9]::INT[], 99)",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-position-absent", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![None]]);
}

#[test]
fn should_reject_element_search_in_multidimensional_arrays_with_0a000() {
    // Arrange
    let position = fixture::execute_cycle(
        "SELECT array_position(ARRAY[[1,2],[3,4]], 2)",
        None,
        0,
        false,
    );
    let positions = fixture::execute_cycle(
        "SELECT array_positions(ARRAY[[1,2],[3,4]], 2)",
        None,
        0,
        false,
    );
    let recovery = fixture::execute_cycle("SELECT 47 AS recovery", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-807-multidimensional-array-search-recovery",
        &[],
        vec![position, positions, recovery],
    );

    // Assert
    for frames in &batches[..2] {
        assert_eq!(wire::error_code(frames).as_deref(), Some("0A000"));
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'E').count(), 1);
        let fields = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .expect("multidimensional array search ErrorResponse");
        assert!(fields.iter().any(|(kind, value)| {
            *kind == 'M'
                && value == "searching for elements in multidimensional arrays is not supported"
        }));
        assert!(fixture::data_row_payloads(frames).is_empty());
    }
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("47".to_owned())]]
    );
}
