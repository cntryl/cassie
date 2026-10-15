//! Correlated scalar-subquery execution count and value identity for #762.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn assert_correlated_evaluation_count(
    correlated: &[(u8, Vec<u8>)],
    next_sequence_value: &[(u8, Vec<u8>)],
) {
    assert_eq!(
        wire::error_code(correlated),
        None,
        "correlated scalar-subquery ErrorResponse: {:?}",
        correlated
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let description = correlated
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("correlated scalar-subquery RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 20]
    );
    assert_eq!(
        wire::data_rows(correlated),
        vec![
            vec![Some("1".to_owned()), Some("2".to_owned())],
            vec![Some("2".to_owned()), Some("3".to_owned())],
            vec![Some("3".to_owned()), Some("4".to_owned())],
        ],
        "nextval must run once for each matching outer row"
    );
    assert_eq!(wire::error_code(next_sequence_value), None);
    assert_eq!(
        wire::data_rows(next_sequence_value),
        vec![vec![Some("5".to_owned())]],
        "three correlated evaluations must advance the sequence three times"
    );
}

#[test]
fn should_evaluate_per_required_row_given_a_correlated_subquery_when_outer_rows_are_projected() {
    // Arrange
    let batches = run_script(
        "correlated-scalar-subquery-evaluation-count",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE SEQUENCE issue_762_correlated_eval_sequence START WITH 1",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_correlated_eval_outer (id INT4 PRIMARY KEY)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_762_correlated_eval_inner (id INT4 PRIMARY KEY)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_correlated_eval_outer VALUES (1), (2), (3)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_762_correlated_eval_inner VALUES (1), (2), (3)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT outer_row.id, outer_row.id * 10 AS ordinary_value \
                 FROM issue_762_correlated_eval_outer AS outer_row ORDER BY outer_row.id",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT nextval('issue_762_correlated_eval_sequence') AS sequence_control",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT outer_row.id, \
                        (SELECT nextval('issue_762_correlated_eval_sequence') \
                         FROM issue_762_correlated_eval_inner AS inner_row \
                         WHERE inner_row.id = outer_row.id) AS sequence_value \
                 FROM issue_762_correlated_eval_outer AS outer_row ORDER BY outer_row.id",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT nextval('issue_762_correlated_eval_sequence') AS next_sequence_value",
            )]),
        ],
    );

    // Act
    let setup = &batches[..5];
    let table_control = &batches[5];
    let sequence_control = &batches[6];
    let correlated = &batches[7];
    let next_sequence_value = &batches[8];

    // Assert
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "correlated subquery setup ErrorResponses: {:?}",
        setup
            .iter()
            .flat_map(|frames| frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1)))
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::error_code(table_control), None);
    assert_eq!(
        wire::data_rows(table_control),
        vec![
            vec![Some("1".to_owned()), Some("10".to_owned())],
            vec![Some("2".to_owned()), Some("20".to_owned())],
            vec![Some("3".to_owned()), Some("30".to_owned())],
        ]
    );
    assert_eq!(wire::error_code(sequence_control), None);
    assert_eq!(
        wire::data_rows(sequence_control),
        vec![vec![Some("1".to_owned())]],
        "direct nextval control proves sequence setup and function support"
    );
    assert_correlated_evaluation_count(correlated, next_sequence_value);
}
