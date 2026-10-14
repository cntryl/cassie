use super::{run_script, wire, Step};

#[test]
fn should_aggregate_numeric_special_values_with_pgwire_metadata() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_numeric_special_aggregate_values (value NUMERIC)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_numeric_special_aggregate_values VALUES \
             ('-Infinity'), (5.25), ('Infinity'), ('NaN')",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT SUM(value), AVG(value), MIN(value), MAX(value) \
             FROM issue_812_numeric_special_aggregate_values",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-special-aggregates", steps);

    // Assert
    let result = &batches[2];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("numeric aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| {
                (
                    column.type_oid,
                    column.type_size,
                    column.type_mod,
                    column.format_code,
                )
            })
            .collect::<Vec<_>>(),
        vec![(1700, -1, -1, 0); 4]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("NaN".to_owned()),
            Some("NaN".to_owned()),
            Some("-Infinity".to_owned()),
            Some("NaN".to_owned()),
        ]]
    );
}
