//! Existing COALESCE arrays retain modifiers only when every argument agrees.
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_describe_untyped_null_coalesce_arrays_without_element_modifiers() {
    // Arrange
    let cases = [
        (
            "SELECT coalesce(ARRAY['x'::CHAR(3)],NULL)",
            1014,
            "{\"x  \"}",
        ),
        (
            "SELECT coalesce(NULL,ARRAY['x'::CHAR(3)])",
            1014,
            "{\"x  \"}",
        ),
        ("SELECT coalesce(ARRAY['x'::VARCHAR(3)],NULL)", 1015, "{x}"),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("coalesce-array-null-modifiers", &[], cycles);

    // Assert
    for ((sql, oid, value), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, -1, -1), 0, true);
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![Some(value.into())]],
            "{sql}"
        );
    }
}

#[test]
fn should_preserve_coalesce_array_modifiers_for_matching_typed_nulls() {
    // Arrange
    let cases = [
        (
            "SELECT coalesce(ARRAY['x'::CHAR(3)],NULL::CHAR(3)[])",
            1014,
            Some("{\"x  \"}"),
        ),
        (
            "SELECT coalesce(NULL::CHAR(3)[],NULL::CHAR(3)[])",
            1014,
            None,
        ),
        (
            "SELECT coalesce(ARRAY['x'::VARCHAR(3)],NULL::VARCHAR(3)[])",
            1015,
            Some("{x}"),
        ),
    ];
    let cycles = cases
        .iter()
        .map(|(sql, _, _)| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("coalesce-array-typed-null-modifiers", &[], cycles);

    // Assert
    for ((sql, oid, value), frames) in cases.into_iter().zip(&batches) {
        fixture::assert_success(frames);
        fixture::assert_single_column_descriptors(frames, (oid, -1, 7), 0, true);
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![value.map(str::to_owned)]],
            "{sql}"
        );
    }
}
