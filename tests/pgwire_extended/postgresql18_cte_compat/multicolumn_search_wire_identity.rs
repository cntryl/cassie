use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_return_typed_multicolumn_depth_first_search_paths() {
    // Arrange
    let sql = "WITH RECURSIVE edges(source_node, source_label, target_node, target_label) AS (\
        VALUES (1::INT4, 'A'::TEXT, 2::INT4, 'B'::TEXT),\
               (1, 'A', 3, 'C'),\
               (2, 'B', 4, 'D'),\
               (3, 'C', 5, 'E')\
    ), walk(node, label, depth) AS (\
        SELECT 1::INT4, 'A'::TEXT, 0::INT4 \
        UNION ALL \
        SELECT edge.target_node, edge.target_label, parent.depth + 1 \
        FROM edges AS edge JOIN walk AS parent \
          ON edge.source_node = parent.node AND edge.source_label = parent.label\
    ) SEARCH DEPTH FIRST BY node, label SET sequence \
    SELECT node, label, depth, sequence FROM walk ORDER BY sequence";
    let batches = run_script(
        "issue-801-multicolumn-search-wire-identity",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("multi-column SEARCH RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["node", "label", "depth", "sequence"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 25, 23, 2287]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("1".to_owned()),
                Some("A".to_owned()),
                Some("0".to_owned()),
                Some(r#"{"(1,A)"}"#.to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("B".to_owned()),
                Some("1".to_owned()),
                Some(r#"{"(1,A)","(2,B)"}"#.to_owned()),
            ],
            vec![
                Some("4".to_owned()),
                Some("D".to_owned()),
                Some("2".to_owned()),
                Some(r#"{"(1,A)","(2,B)","(4,D)"}"#.to_owned()),
            ],
            vec![
                Some("3".to_owned()),
                Some("C".to_owned()),
                Some("1".to_owned()),
                Some(r#"{"(1,A)","(3,C)"}"#.to_owned()),
            ],
            vec![
                Some("5".to_owned()),
                Some("E".to_owned()),
                Some("2".to_owned()),
                Some(r#"{"(1,A)","(3,C)","(5,E)"}"#.to_owned()),
            ],
        ]
    );
}
