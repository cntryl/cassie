//! Multi-column CYCLE mark and record-array identity over PostgreSQL wire.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_return_typed_cycle_marks_for_multicolumn_keys() {
    // Arrange
    let sql = "WITH RECURSIVE edges(source_node, source_label, target_node, target_label) AS (\
        VALUES (1::INT4, 'A'::TEXT, 2::INT4, 'B'::TEXT),\
               (2::INT4, 'B'::TEXT, 1::INT4, 'C'::TEXT),\
               (1::INT4, 'C'::TEXT, 2::INT4, 'D'::TEXT),\
               (2::INT4, 'D'::TEXT, 1::INT4, 'A'::TEXT)\
    ), walk(node, label, depth) AS (\
        SELECT 1::INT4, 'A'::TEXT, 0::INT4 \
        UNION ALL \
        SELECT edge.target_node, edge.target_label, parent.depth + 1 \
        FROM edges AS edge JOIN walk AS parent \
          ON edge.source_node = parent.node AND edge.source_label = parent.label\
    ) CYCLE node, label SET marker TO 'cycle' DEFAULT 'clear' USING path \
    SELECT node, label, depth, marker, path FROM walk ORDER BY depth";
    let batches = run_script(
        "issue-801-multicolumn-cycle-wire-identity",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("multi-column CYCLE RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["node", "label", "depth", "marker", "path"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 25, 23, 25, 2287]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![
                Some("1".to_owned()),
                Some("A".to_owned()),
                Some("0".to_owned()),
                Some("clear".to_owned()),
                Some("{\"(1,A)\"}".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("B".to_owned()),
                Some("1".to_owned()),
                Some("clear".to_owned()),
                Some("{\"(1,A)\",\"(2,B)\"}".to_owned()),
            ],
            vec![
                Some("1".to_owned()),
                Some("C".to_owned()),
                Some("2".to_owned()),
                Some("clear".to_owned()),
                Some("{\"(1,A)\",\"(2,B)\",\"(1,C)\"}".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("D".to_owned()),
                Some("3".to_owned()),
                Some("clear".to_owned()),
                Some("{\"(1,A)\",\"(2,B)\",\"(1,C)\",\"(2,D)\"}".to_owned()),
            ],
            vec![
                Some("1".to_owned()),
                Some("A".to_owned()),
                Some("4".to_owned()),
                Some("cycle".to_owned()),
                Some("{\"(1,A)\",\"(2,B)\",\"(1,C)\",\"(2,D)\",\"(1,A)\"}".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_describe_generated_cycle_columns_for_a_parameterized_walk() {
    // Arrange
    let sql = "WITH RECURSIVE walk(id) AS (SELECT $1::INT UNION ALL \
               SELECT id FROM walk WHERE id=1) CYCLE id SET cy USING path \
               SELECT id,cy,path FROM walk ORDER BY cy";
    let frames = vec![
        wire::parse_frame_with_types("", sql, &[23]),
        wire::describe_statement_frame(""),
        wire::bind_frame_with_formats("", "", &[0], &[Some(b"1")], &[]),
        wire::describe_portal_frame(""),
        wire::execute_frame(""),
        wire::sync_frame(),
    ];

    // Act
    let batches = run_script("cte-cycle-bound-seed", vec![Step::ThroughReady(frames)]);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let descriptions = result
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(descriptions.len(), 2);
    for columns in descriptions {
        assert_eq!(
            columns
                .iter()
                .map(|c| (c.name.as_str(), c.type_oid))
                .collect::<Vec<_>>(),
            [("id", 23), ("cy", 16), ("path", 2287)]
        );
    }
    assert_eq!(
        wire::data_rows(result),
        [
            vec![Some("1".into()), Some("f".into()), Some("{(1)}".into())],
            vec![Some("1".into()), Some("t".into()), Some("{(1),(1)}".into())],
        ]
    );
}

#[test]
fn should_apply_cycle_field_oids_only_to_their_source() {
    // Arrange
    let prefix = "WITH RECURSIVE walk(id) AS (SELECT $1::INT UNION ALL \
                  SELECT id FROM walk WHERE id=1) CYCLE id SET cy USING path, \
                  siblings(path,cy) AS (SELECT 2.5::NUMERIC,9::INT) ";
    let cases = [
        (
            "SELECT w.path AS x,s.path AS x,w.cy AS y,s.cy AS y \
             FROM walk w CROSS JOIN siblings s ORDER BY w.cy",
            vec![("x", 2287), ("x", 1700), ("y", 16), ("y", 23)],
            vec!["{(1)}", "2.5", "f", "9"],
        ),
        (
            "SELECT s.*,w.* FROM siblings s CROSS JOIN walk w ORDER BY w.cy",
            vec![
                ("path", 1700),
                ("cy", 23),
                ("id", 23),
                ("cy", 16),
                ("path", 2287),
            ],
            vec!["2.5", "9", "1", "f", "{(1)}"],
        ),
    ];
    let mut steps = Vec::new();
    for (select, _, _) in &cases {
        let sql = format!("{prefix}{select}");
        steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
            &sql.replace("$1", "1"),
        )]));
        steps.push(Step::ThroughReady(vec![
            wire::parse_frame_with_types("", &sql, &[23]),
            wire::describe_statement_frame(""),
            wire::bind_frame_with_formats("", "", &[0], &[Some(b"1")], &[]),
            wire::describe_portal_frame(""),
            wire::execute_frame(""),
            wire::sync_frame(),
        ]));
    }

    // Act
    let batches = run_script("cte-cycle-field-source", steps);

    // Assert
    for ((_, expected, first_row), pair) in cases.into_iter().zip(batches.chunks_exact(2)) {
        for (batch, count) in pair.iter().zip([1, 2]) {
            assert_eq!(wire::error_code(batch), None);
            let descriptions = batch
                .iter()
                .filter(|(tag, _)| *tag == b'T')
                .collect::<Vec<_>>();
            assert_eq!(descriptions.len(), count);
            for (_, body) in descriptions {
                let columns = wire::parse_row_description(body);
                assert_eq!(
                    columns
                        .iter()
                        .map(|c| (c.name.as_str(), c.type_oid))
                        .collect::<Vec<_>>(),
                    expected
                );
            }
            let rows = wire::data_rows(batch);
            assert_eq!(rows.len(), 2);
            assert_eq!(
                rows[0].iter().map(Option::as_deref).collect::<Vec<_>>(),
                first_row.iter().copied().map(Some).collect::<Vec<_>>()
            );
        }
    }
}
