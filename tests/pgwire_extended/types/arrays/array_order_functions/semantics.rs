#[test]
fn should_reverse_array_elements_in_order() {
    // Arrange
    let sql = "SELECT array_reverse(ARRAY[1,NULL,3]) AS reversed";

    // Act
    let frames = run_array_query(sql, "issue-807-array-reverse-null-element");

    // Assert
    fixture::assert_success(&frames);
    fixture::assert_single_column_descriptors(&frames, (1_007, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![Some("{3,NULL,1}".to_owned())]]
    );
}

#[test]
fn should_shuffle_arrays_without_losing_elements() {
    // Arrange
    let sql = "SELECT cardinality(shuffled), \
                      (SELECT count(DISTINCT value) FROM unnest(shuffled) AS values(value)), \
                      (SELECT count(*) FROM unnest(shuffled) AS values(value) \
                       WHERE value = ANY(ARRAY[1,2,3,4])) \
               FROM (SELECT array_shuffle(ARRAY[1,2,3,4]) AS shuffled) AS input";

    // Act
    let frames = run_array_query(sql, "issue-807-array-shuffle-preserves-bag");

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(
        wire::parse_row_description(
            &frames
                .iter()
                .find(|(tag, _)| *tag == b'T')
                .expect("RowDescription")
                .1
        )
        .iter()
        .map(|column| column.type_oid)
        .collect::<Vec<_>>(),
        vec![23, 20, 20]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("4".to_owned()),
            Some("4".to_owned()),
            Some("4".to_owned()),
        ]]
    );
}

#[test]
fn should_sample_the_requested_number_of_distinct_array_elements() {
    // Arrange
    let sql = "SELECT cardinality(sample), \
                      (SELECT count(DISTINCT value) FROM unnest(sample) AS values(value)), \
                      (SELECT count(*) FROM unnest(sample) AS values(value) \
                       WHERE value = ANY(ARRAY[1,2,3,4])) \
               FROM (SELECT array_sample(ARRAY[1,2,3,4], 2) AS sample) AS input";

    // Act
    let frames = run_array_query(sql, "issue-807-array-sample-size-and-membership");

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(
        wire::parse_row_description(
            &frames
                .iter()
                .find(|(tag, _)| *tag == b'T')
                .expect("RowDescription")
                .1
        )
        .iter()
        .map(|column| column.type_oid)
        .collect::<Vec<_>>(),
        vec![23, 20, 20]
    );
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![
            Some("2".to_owned()),
            Some("2".to_owned()),
            Some("2".to_owned()),
        ]]
    );
}
