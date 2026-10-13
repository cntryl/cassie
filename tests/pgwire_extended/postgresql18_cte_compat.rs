//! PostgreSQL 18 CTE parameter metadata and Bind behavior tracked by #801.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_describe_typed_parameter_inside_materialized_cte() {
    // Arrange
    let sql =
        "WITH chosen AS MATERIALIZED (SELECT CAST($1 AS BIGINT) AS value) SELECT value FROM chosen";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types("cte_parameter", sql, &[0])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("cte_parameter"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("cte_portal", "cte_parameter", &["42"]),
            wire::describe_portal_frame("cte_portal"),
            wire::execute_frame("cte_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("postgresql18-cte-parameter", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    let parameter_types = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![20]
    );
    let result_columns = batches[1]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    assert_eq!(
        wire::parse_row_description(&result_columns.1)[0].type_oid,
        20
    );
    assert_eq!(
        batches[2].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'2', b'T', b'D', b'C', b'Z']
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}
