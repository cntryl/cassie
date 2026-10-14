//! PostgreSQL 18 aggregate binding errors and functional dependencies over pgwire.

#[path = "postgresql18_aggregate_errors/resource_cancellation.rs"]
mod resource_cancellation;

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

mod boolean_or_type_error;
mod boolean_aggregate_cancellation;
mod distinct_order_by_argument;

#[test]
fn should_apply_primary_key_functional_dependency_for_grouped_aggregates() {
    // Arrange
    let setup = [
        "CREATE TABLE issue_804_grouped_primary_key (id INT PRIMARY KEY, region TEXT, amount INT)",
        "INSERT INTO issue_804_grouped_primary_key VALUES (1, 'east', 2), (2, 'east', 3), (3, 'west', 5)",
        "CREATE TABLE issue_804_grouped_without_key (id INT, region TEXT, amount INT)",
        "INSERT INTO issue_804_grouped_without_key VALUES (1, 'east', 2), (1, 'west', 3)",
    ];
    let mut steps = setup
        .iter()
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect::<Vec<_>>();
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT id, region, SUM(amount) FROM issue_804_grouped_primary_key GROUP BY id ORDER BY id",
    )]));
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT id, region, SUM(amount) FROM issue_804_grouped_without_key GROUP BY id",
    )]));
    let batches = run_script(
        "issue-804-primary-key-grouping-functional-dependency",
        steps,
    );

    // Act
    let valid = &batches[4];
    let invalid = &batches[5];

    // Assert
    assert!(
        batches[..4]
            .iter()
            .all(|batch| wire::error_code(batch).is_none()),
        "create and seed both grouping fixtures"
    );
    assert_eq!(wire::error_code(valid), None);
    let description = valid
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription for the grouped primary-key query");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|field| field.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25, 20]
    );
    assert_eq!(
        wire::data_rows(valid),
        vec![
            vec![
                Some("1".to_owned()),
                Some("east".to_owned()),
                Some("2".to_owned())
            ],
            vec![
                Some("2".to_owned()),
                Some("east".to_owned()),
                Some("3".to_owned())
            ],
            vec![
                Some("3".to_owned()),
                Some("west".to_owned()),
                Some("5".to_owned())
            ],
        ]
    );
    assert_eq!(wire::error_code(invalid).as_deref(), Some("42803"));
    assert!(
        wire::data_rows(invalid).is_empty(),
        "the invalid grouped query must not publish partial rows"
    );
}

#[test]
fn should_reject_non_boolean_boolean_aggregate_arguments_with_42883() {
    // Arrange
    let batches = run_script(
        "issue-805-boolean-aggregate-argument-type",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT BOOL_AND(flag) FROM (VALUES (TRUE), (NULL::BOOLEAN)) AS input(flag)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT BOOL_AND(1)")]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];
    let recovery = &batches[2];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    let description = valid
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription for Boolean aggregate");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("t".to_owned())]]);
    assert_eq!(wire::error_code(invalid).as_deref(), Some("42883"));
    let fields = invalid
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("undefined Boolean aggregate overload ErrorResponse");
    assert!(fields.iter().any(|(kind, message)| {
        *kind == 'M' && message.starts_with("function bool_and(integer) does not exist")
    }));
    assert!(wire::data_rows(invalid).is_empty());
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}

#[test]
fn should_preserve_postgres_syntax_rejection_for_some_aggregate_spelling() {
    // Arrange
    let batches = run_script(
        "issue-805-postgres-some-aggregate-syntax",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT SOME(flag) FROM (VALUES (FALSE), (TRUE), (NULL::BOOLEAN)) AS input(flag)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let invalid = &batches[0];
    let recovery = &batches[1];

    // Assert
    assert_eq!(wire::error_code(invalid).as_deref(), Some("42601"));
    let fields = invalid
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("syntax error for ambiguous SOME aggregate spelling");
    assert!(fields.iter().any(|(kind, message)| {
        *kind == 'M' && message.starts_with("syntax error at or near \"SOME\"")
    }));
    assert!(wire::data_rows(invalid).is_empty());
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}
