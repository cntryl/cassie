#[test]
fn should_reject_duplicate_sql_null_values_in_unique_json_object_aggregates() {
    // Arrange
    let setup = [
        "CREATE TABLE issue_811_unique_json_null_values (key TEXT, value INT, position INT)",
        "INSERT INTO issue_811_unique_json_null_values VALUES ('same',NULL,1),('same',NULL,2),('kept',9,3)",
    ];
    let cases = [
        (
            "json_object_agg_unique",
            "SELECT json_object_agg_unique(key,value ORDER BY position) FROM issue_811_unique_json_null_values",
            "duplicate JSON object key value: \"same\"",
        ),
        (
            "jsonb_object_agg_unique",
            "SELECT jsonb_object_agg_unique(key,value ORDER BY position) FROM issue_811_unique_json_null_values",
            "duplicate JSON object key value",
        ),
        (
            "json_object_agg_unique_strict",
            "SELECT json_object_agg_unique_strict(key,value ORDER BY position) FROM issue_811_unique_json_null_values",
            "duplicate JSON object key value: \"same\"",
        ),
        (
            "jsonb_object_agg_unique_strict",
            "SELECT jsonb_object_agg_unique_strict(key,value ORDER BY position) FROM issue_811_unique_json_null_values",
            "duplicate JSON object key value",
        ),
    ];
    let queries = cases
        .iter()
        .map(|(_, sql, _)| *sql)
        .chain(std::iter::once("SELECT 42"))
        .collect::<Vec<_>>();

    // Act
    let batches = run_queries("issue-811-unique-json-null-duplicates", &setup, &queries);

    // Assert
    let mut mismatches = Vec::new();
    for ((name, _, expected_message), frames) in cases.iter().zip(&batches) {
        let actual_code = wire::error_code(frames);
        if actual_code.as_deref() != Some("22030") {
            mismatches.push(format!("{name} returned SQLSTATE {actual_code:?}"));
            continue;
        }
        let actual_message = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .and_then(|(_, body)| {
                wire::parse_error_fields(body)
                    .into_iter()
                    .find_map(|(key, value)| (key == 'M').then_some(value))
            });
        if actual_message.as_deref() != Some(*expected_message) {
            mismatches.push(format!("{name} returned message {actual_message:?}"));
        }
        if frames
            .iter()
            .any(|(tag, _)| matches!(*tag, b'T' | b'D' | b'C'))
        {
            mismatches.push(format!("{name} published result frames after an error"));
        }
    }

    let recovery = batches.last().expect("post-error query batch");
    assert_eq!(
        wire::error_code(recovery),
        None,
        "connection recovery query"
    );
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("42".to_owned())]]);
    assert!(
        mismatches.is_empty(),
        "unique JSON aggregate NULL duplicate mismatches: {mismatches:?}"
    );
}
