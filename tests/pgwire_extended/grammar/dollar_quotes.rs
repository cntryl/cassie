//! Dollar-quoted SQL literals through the primary simple Query interface.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_dollar_quoted_values_given_literal_delimiters_when_a_following_query_executes() {
    // Arrange
    let sql = "SELECT $té$多 it's; /* keep */ -- literal$té$ AS tagged, \
               $$a,b (x) + 1::INT4$$ AS punctuation, \
               coalesce(CASE WHEN TRUE THEN $$END's; WHEN /* keep */$$ \
               ELSE 'fallback' END, 'outside') AS case_value; \
               SELECT 9::INT4 AS after_literal";

    // Act
    let batches = run_script(
        "issue-796-dollar-quote-wire-values",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames), None);
    let descriptions = frames
        .iter()
        .filter(|frame| frame.0 == b'T')
        .map(|frame| wire::parse_row_description(&frame.1))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2);
    assert_eq!(
        descriptions[0]
            .iter()
            .map(|column| (column.type_oid, column.type_mod, column.format_code))
            .collect::<Vec<_>>(),
        vec![(25, -1, 0), (25, -1, 0), (25, -1, 0)]
    );
    assert_eq!(descriptions[1][0].name, "after_literal");
    assert_eq!(descriptions[1][0].type_oid, 23);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("多 it's; /* keep */ -- literal".to_owned()),
                Some("a,b (x) + 1::INT4".to_owned()),
                Some("END's; WHEN /* keep */".to_owned()),
            ],
            vec![Some("9".to_owned())],
        ]
    );
    assert_eq!(frames.iter().filter(|frame| frame.0 == b'C').count(), 2);
    assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
}

#[test]
fn should_reject_the_whole_batch_given_an_unmatched_dollar_quote_when_ddl_precedes_the_literal() {
    // Arrange
    let malformed = "CREATE TABLE issue_796_dollar_unclosed (n INT4); \
                     SELECT $tag$unterminated";

    // Act
    let batches = run_script(
        "issue-796-dollar-quote-wire-recovery",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(malformed)]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT * FROM issue_796_dollar_unclosed",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 17::INT4 AS recovered",
            )]),
        ],
    );

    // Assert
    let rejected = &batches[0];
    assert_eq!(wire::error_code(rejected).as_deref(), Some("42601"));
    assert_eq!(wire::data_rows(rejected), Vec::<Vec<Option<String>>>::new());
    assert!(rejected.iter().all(|frame| frame.0 != b'C'));
    let missing_relation = &batches[1];
    assert_eq!(wire::error_code(missing_relation).as_deref(), Some("42P01"));
    assert_eq!(
        wire::data_rows(missing_relation),
        Vec::<Vec<Option<String>>>::new()
    );
    for frames in [&batches[0], &batches[1], &batches[2]] {
        assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
        assert_eq!(frames.last().expect("ReadyForQuery").1.as_slice(), b"I");
    }
    let recovered = &batches[2];
    assert_eq!(wire::error_code(recovered), None);
    assert_eq!(
        wire::data_rows(recovered),
        vec![vec![Some("17".to_owned())]]
    );
    let description = recovered
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("recovery RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns[0].name, "recovered");
    assert_eq!(columns[0].type_oid, 23);
}
