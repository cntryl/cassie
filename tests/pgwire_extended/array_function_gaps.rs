//! Direct PostgreSQL array function contracts not covered by the scalar-array matrix.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;
#[test]
fn should_report_two_dimensions_for_nested_arrays() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_NDIMS($1::INT[]) AS dimensions",
        Some((1_007, 0, Some(b"{{1,2},{3,4}}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-ndims-nested", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("2".to_owned())]]);
}

#[test]
fn should_report_one_dimension_for_nondefault_array_bounds() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_NDIMS($1::INT[]) AS dimensions",
        Some((1_007, 0, Some(b"[0:2]={7,NULL,9}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-ndims-nondefault-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (23, 4, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("1".to_owned())]]);
}

#[test]
fn should_prepend_elements_while_preserving_nondefault_array_bounds() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ARRAY_PREPEND(0, $1::INT[])::TEXT",
        Some((1_007, 0, Some(b"[0:2]={7,NULL,9}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-807-array-prepend-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:3]={0,7,NULL,9}".to_owned())]]
    );
}

#[test]
fn should_concatenate_arrays_while_preserving_postgresql_bounds() {
    // Arrange
    let cycle = vec![
        wire::parse_frame_with_types(
            "",
            "SELECT ARRAY_CAT($1::INT[], $2::INT[])::TEXT",
            &[1_007, 1_007],
        ),
        wire::bind_frame_with_formats(
            "",
            "",
            &[0, 0],
            &[Some(b"[0:1]={1,2}"), Some(b"[4:5]={3,4}")],
            &[],
        ),
        wire::execute_frame(""),
        wire::sync_frame(),
    ];

    // Act
    let batches = fixture::run_cycles("issue-807-array-cat-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:3]={1,2,3,4}".to_owned())]]
    );
}

#[test]
fn should_fill_multidimensional_arrays_from_explicit_lower_bounds() {
    // Arrange
    let cycle = vec![
        wire::parse_frame_with_types(
            "",
            "SELECT ARRAY_FILL(7, $1::INT[], $2::INT[])::TEXT",
            &[1_007, 1_007],
        ),
        wire::bind_frame_with_formats("", "", &[0, 0], &[Some(b"{2,3}"), Some(b"{0,-1}")], &[]),
        wire::execute_frame(""),
        wire::sync_frame(),
    ];

    // Act
    let batches = fixture::run_cycles("issue-807-array-fill-lower-bounds", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[0:1][-1:1]={{7,7,7},{7,7,7}}".to_owned())]]
    );
}
