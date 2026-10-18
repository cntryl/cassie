//! PostgreSQL 18 CTE parameter metadata and Bind behavior tracked by #801.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "../ctes/multicolumn_cycle_wire_identity.rs"]
mod multicolumn_cycle_wire_identity;
#[path = "../sequences/multicolumn_search_wire_identity.rs"]
mod multicolumn_search_wire_identity;

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

#[test]
fn should_observe_filter_pushdown_across_cte_materialization_modes() {
    // Arrange
    let materialized_sql = "WITH chosen AS MATERIALIZED (SELECT x, 10 / x AS quotient FROM (VALUES (0), (1)) AS input(x)) SELECT x, quotient FROM chosen WHERE x <> 0";
    let not_materialized_sql = "WITH chosen AS NOT MATERIALIZED (SELECT x, 10 / x AS quotient FROM (VALUES (0), (1)) AS input(x)) SELECT x, quotient FROM chosen WHERE x <> 0";
    let batches = run_script(
        "postgresql18-cte-materialization-observable",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(materialized_sql)]),
            Step::ThroughReady(vec![wire::simple_query_frame(not_materialized_sql)]),
        ],
    );

    // Act
    let materialized = &batches[0];
    let not_materialized = &batches[1];

    // Assert
    // PostgreSQL 18.6 materializes both VALUES rows before filtering, so the
    // division by zero is visible. NOT MATERIALIZED permits the filter to be
    // pushed into the inlined query and returns the nonzero row.
    assert_eq!(wire::error_code(materialized).as_deref(), Some("22012"));
    assert!(!materialized.iter().any(|(tag, _)| *tag == b'D'));
    assert_eq!(wire::error_code(not_materialized), None);
    let description = not_materialized
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("NOT MATERIALIZED RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23]
    );
    assert_eq!(
        wire::data_rows(not_materialized),
        vec![vec![Some("1".to_owned()), Some("10".to_owned())]]
    );
}

#[test]
fn should_return_branch_sensitive_search_sequences_over_pgwire() {
    // Arrange
    let common = "WITH RECURSIVE tree(parent, node) AS (VALUES (NULL::INT4, 1::INT4), (1, 2), (1, 3), (2, 4), (3, 5)), walk(node) AS (SELECT node FROM tree WHERE parent IS NULL UNION ALL SELECT child.node FROM tree AS child JOIN walk AS parent ON child.parent = parent.node)";
    let depth_first = format!(
        "{common} SEARCH DEPTH FIRST BY node SET sequence SELECT node, sequence FROM walk ORDER BY sequence"
    );
    let breadth_first = format!(
        "{common} SEARCH BREADTH FIRST BY node SET sequence SELECT node, sequence FROM walk ORDER BY sequence"
    );
    let batches = run_script(
        "postgresql18-cte-search-branch-values",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(&depth_first)]),
            Step::ThroughReady(vec![wire::simple_query_frame(&breadth_first)]),
        ],
    );

    // Act
    let depth_first = &batches[0];
    let breadth_first = &batches[1];

    // Assert
    assert_eq!(wire::error_code(depth_first), None);
    assert_eq!(wire::error_code(breadth_first), None);
    let depth_description = depth_first
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("depth-first SEARCH RowDescription");
    assert_eq!(
        wire::parse_row_description(&depth_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 2287]
    );
    assert_eq!(
        wire::data_rows(depth_first),
        vec![
            vec![Some("1".to_owned()), Some("{(1)}".to_owned())],
            vec![Some("2".to_owned()), Some("{(1),(2)}".to_owned())],
            vec![Some("4".to_owned()), Some("{(1),(2),(4)}".to_owned())],
            vec![Some("3".to_owned()), Some("{(1),(3)}".to_owned())],
            vec![Some("5".to_owned()), Some("{(1),(3),(5)}".to_owned())],
        ]
    );
    let breadth_description = breadth_first
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("breadth-first SEARCH RowDescription");
    assert_eq!(
        wire::parse_row_description(&breadth_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 2249]
    );
    assert_eq!(
        wire::data_rows(breadth_first),
        vec![
            vec![Some("1".to_owned()), Some("(0,1)".to_owned())],
            vec![Some("2".to_owned()), Some("(1,2)".to_owned())],
            vec![Some("3".to_owned()), Some("(1,3)".to_owned())],
            vec![Some("4".to_owned()), Some("(2,4)".to_owned())],
            vec![Some("5".to_owned()), Some("(2,5)".to_owned())],
        ]
    );
}

#[test]
fn should_report_recursive_anchor_errors_with_postgres_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-801-recursive-anchor-error",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "WITH RECURSIVE walk(n) AS (\
                    SELECT 1 UNION ALL SELECT n + 1 FROM walk WHERE n < 2\
                 ) SELECT n FROM walk ORDER BY n",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "WITH RECURSIVE walk(n) AS (SELECT n + 1 FROM walk) SELECT n FROM walk",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];
    let recovery = &batches[2];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(
        wire::data_rows(valid),
        vec![vec![Some("1".to_owned())], vec![Some("2".to_owned())]]
    );
    assert_eq!(
        wire::error_code(invalid).as_deref(),
        Some("42P19"),
        "recursive anchor SQLSTATE; frames: {invalid:?}"
    );
    let fields = invalid
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("recursive anchor ErrorResponse");
    assert!(
        fields.iter().any(|(kind, value)| {
            *kind == 'M'
                && value.starts_with(
                    "recursive query \"walk\" does not have the form non-recursive-term UNION",
                )
        }),
        "invalid anchor must reach PostgreSQL's recursive-form validation: {fields:?}"
    );
    assert_eq!(wire::data_rows(invalid), Vec::<Vec<Option<String>>>::new());
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}
