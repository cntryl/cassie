//! PostgreSQL 18 aggregate binding errors and functional dependencies over pgwire.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

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
