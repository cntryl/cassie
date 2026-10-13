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

#[test]
fn should_return_postgres_typed_search_cycle_columns() {
    // Arrange
    let batches = run_script(
        "postgresql18-cte-search-cycle-values",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_801_wire_cycle_edges (source BIGINT, target BIGINT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_801_wire_cycle_edges VALUES (1, 2), (2, 1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "WITH RECURSIVE walk(node) AS (SELECT 1::BIGINT UNION ALL SELECT node + 1 FROM walk WHERE node < 3) SEARCH DEPTH FIRST BY node SET sequence SELECT node, sequence FROM walk ORDER BY node",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "WITH RECURSIVE walk(node) AS (SELECT 1::BIGINT UNION ALL SELECT edge.target FROM issue_801_wire_cycle_edges AS edge JOIN walk AS parent ON edge.source = parent.node) CYCLE node SET is_cycle USING path SELECT node, is_cycle, path FROM walk ORDER BY node, is_cycle",
            )]),
        ],
    );

    // Act
    let search = &batches[2];
    let cycle = &batches[3];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(wire::error_code(search), None);
    assert_eq!(wire::error_code(cycle), None);
    let search_description = search
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("SEARCH RowDescription");
    assert_eq!(
        wire::parse_row_description(&search_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 2287]
    );
    assert_eq!(
        wire::data_rows(search),
        vec![
            vec![Some("1".to_owned()), Some("{(1)}".to_owned())],
            vec![Some("2".to_owned()), Some("{(1),(2)}".to_owned())],
            vec![Some("3".to_owned()), Some("{(1),(2),(3)}".to_owned())],
        ]
    );
    let cycle_description = cycle
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("CYCLE RowDescription");
    assert_eq!(
        wire::parse_row_description(&cycle_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 16, 2287]
    );
    assert_eq!(
        wire::data_rows(cycle),
        vec![
            vec![
                Some("1".to_owned()),
                Some("f".to_owned()),
                Some("{(1)}".to_owned()),
            ],
            vec![
                Some("1".to_owned()),
                Some("t".to_owned()),
                Some("{(1),(2),(1)}".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("f".to_owned()),
                Some("{(1),(2)}".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_return_postgres_typed_breadth_first_search_sequences() {
    // Arrange
    let batches = run_script(
        "postgresql18-cte-search-breadth-first-values",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "WITH RECURSIVE walk(node) AS (SELECT 1::BIGINT UNION ALL SELECT node + 1 FROM walk WHERE node < 3) SEARCH BREADTH FIRST BY node SET sequence SELECT node, sequence FROM walk ORDER BY sequence",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("breadth-first SEARCH RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 2249]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("1".to_owned()), Some("(0,1)".to_owned())],
            vec![Some("2".to_owned()), Some("(1,2)".to_owned())],
            vec![Some("3".to_owned()), Some("(2,3)".to_owned())],
        ]
    );
}
