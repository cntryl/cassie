#[test]
fn should_evaluate_jsonpath_range_arithmetic_predicates() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonpath_range_values (array_document JSONB, scalar_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonpath_range_values VALUES ('[10,20,30,40]', '3')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT array_document @? '$[1 to 2] ? (@ % 2 == 0 || @ + 1 == 31)'::JSONPATH, \
         scalar_document @@ '$ + 1 == 4 && !($ == 5)'::JSONPATH, \
         jsonb_path_query_array(array_document, '$[1 to 2] ? (@ + 1 == 31)'::JSONPATH)::TEXT \
         FROM issue_809_jsonpath_range_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-range-arithmetic", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath range and arithmetic RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("[30]".to_owned()),
        ]]
    );
}

#[test]
fn should_compare_jsonpath_date_time_item_methods() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonpath_temporal_methods (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonpath_temporal_methods VALUES \
             ('{\"d\":\"2024-03-10\",\"t\":\"12:34:56\", \
               \"tz\":\"12:34:56-05:00\",\"ts\":\"2024-03-10T12:34:56\", \
               \"tstz\":\"2024-03-10T12:34:56-05:00\"}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document @@ '$.d.date() == \"2024-03-10\".date() \
         && $.t.time() == \"12:34:56\".time() \
         && $.tz.time_tz() == \"12:34:56-05:00\".time_tz() \
         && $.ts.timestamp() == \"2024-03-10T12:34:56\".timestamp() \
         && $.tstz.timestamp_tz() == \"2024-03-10T12:34:56-05:00\".timestamp_tz()'::JSONPATH \
         FROM issue_809_jsonpath_temporal_methods",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-temporal-methods", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (16, 1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}
