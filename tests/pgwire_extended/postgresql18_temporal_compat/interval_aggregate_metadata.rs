use super::{binary_row, run_script, wire, Step};

#[test]
fn should_return_interval_aggregates_with_postgresql_metadata() {
    // Arrange
    let statement = "interval_aggregate_results";
    let sql = "SELECT SUM(value), AVG(value) FROM issue_814_interval_aggregate_wire";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET IntervalStyle = 'postgres'",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_814_interval_aggregate_wire (value INTERVAL)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_814_interval_aggregate_wire VALUES (INTERVAL '1 day'), (INTERVAL '2 days')",
        )]),
        Step::ParseFlush(wire::parse_frame(statement, sql)),
        Step::ThroughReady(vec![
            wire::describe_statement_frame(statement),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("interval_aggregate_text", statement, &[], &[], &[0]),
            wire::describe_portal_frame("interval_aggregate_text"),
            wire::execute_frame("interval_aggregate_text"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats("interval_aggregate_binary", statement, &[], &[], &[1]),
            wire::describe_portal_frame("interval_aggregate_binary"),
            wire::execute_frame("interval_aggregate_binary"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-interval-aggregate-metadata", steps);

    // Assert
    assert!(batches
        .iter()
        .all(|batch| wire::error_code(batch).is_none()));
    for (batch, format_code) in [(&batches[4], 0), (&batches[5], 0), (&batches[6], 1)] {
        let description = batch
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("interval aggregate RowDescription");
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
            vec![(1186, 16, -1, format_code); 2]
        );
    }
    assert_eq!(
        wire::data_rows(&batches[5]),
        vec![vec![
            Some("3 days".to_owned()),
            Some("1 day 12:00:00".to_owned()),
        ]]
    );
    let sum = [
        0_i64.to_be_bytes().as_slice(),
        3_i32.to_be_bytes().as_slice(),
        0_i32.to_be_bytes().as_slice(),
    ]
    .concat();
    let average = [
        43_200_000_000_i64.to_be_bytes().as_slice(),
        1_i32.to_be_bytes().as_slice(),
        0_i32.to_be_bytes().as_slice(),
    ]
    .concat();
    assert_eq!(binary_row(&batches[6]), vec![sum, average]);
}
