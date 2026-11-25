#[test]
fn should_omit_sql_null_values_in_unique_jsonb_object_aggregation() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_811_unique_object_values (key TEXT, value INT)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_811_unique_object_values VALUES ('a', 1), ('b', NULL)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_object_agg_unique_strict(key, value) FROM issue_811_unique_object_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-811-jsonb-object-aggregate-unique-strict",
        &setup,
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (3802, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": 1}".to_owned())]]
    );
}

#[test]
fn should_construct_jsonb_arrays_from_scalar_plus_sql_null_inputs() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_build_array_values (value INTEGER)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_build_array_values VALUES (42)",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_build_array(value, NULL)::TEXT FROM issue_810_build_array_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-build-array", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[42, null]".to_owned())]]
    );
}
