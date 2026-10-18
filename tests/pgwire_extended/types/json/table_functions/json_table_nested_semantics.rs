//! Recursive and sibling `JSON_TABLE` `NESTED PATH` contracts over pgwire.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn column_type_oids(frames: &fixture::Frames) -> Vec<i32> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(&description.1)
        .iter()
        .map(|column| column.type_oid)
        .collect()
}

#[test]
fn should_recurse_through_two_json_table_nested_paths() {
    // Arrange
    let sql = r#"SELECT region, team_ordinal, team, member_ordinal, member_id, member_name
        FROM JSON_TABLE(
            '[{"region":"north","teams":[{"name":"compiler","members":[{"id":1,"name":"Ada"},{"id":2,"name":"Linus"}]},{"name":"planner","members":[{"id":3,"name":"Grace"}]}]}]'::JSONB,
            '$[*]' COLUMNS (
                region TEXT PATH '$.region',
                NESTED PATH '$.teams[*]' COLUMNS (
                    team_ordinal FOR ORDINALITY,
                    team TEXT PATH '$.name',
                    NESTED PATH '$.members[*]' COLUMNS (
                        member_ordinal FOR ORDINALITY,
                        member_id INTEGER PATH '$.id',
                        member_name TEXT PATH '$.name'
                    )
                )
            )
        ) AS expanded
        ORDER BY team_ordinal, member_ordinal"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-829-recursive-json-table-nested-path",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![25, 20, 25, 20, 23, 25]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("north".to_owned()),
                Some("1".to_owned()),
                Some("compiler".to_owned()),
                Some("1".to_owned()),
                Some("1".to_owned()),
                Some("Ada".to_owned()),
            ],
            vec![
                Some("north".to_owned()),
                Some("1".to_owned()),
                Some("compiler".to_owned()),
                Some("2".to_owned()),
                Some("2".to_owned()),
                Some("Linus".to_owned()),
            ],
            vec![
                Some("north".to_owned()),
                Some("2".to_owned()),
                Some("planner".to_owned()),
                Some("1".to_owned()),
                Some("3".to_owned()),
                Some("Grace".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_union_sibling_json_table_paths_with_independent_ordinality() {
    // Arrange
    let sql = r#"SELECT department_id, color_ordinal, color, label_ordinal, label
        FROM JSON_TABLE(
            '[{"id":7,"colors":["red","blue"],"labels":["urgent","new"]}]'::JSONB,
            '$[*]' COLUMNS (
                department_id INTEGER PATH '$.id',
                NESTED PATH '$.colors[*]' COLUMNS (
                    color_ordinal FOR ORDINALITY,
                    color TEXT PATH '$'
                ),
                NESTED PATH '$.labels[*]' COLUMNS (
                    label_ordinal FOR ORDINALITY,
                    label TEXT PATH '$'
                )
            )
        ) AS expanded
        ORDER BY color_ordinal NULLS LAST, label_ordinal NULLS LAST"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-829-json-table-sibling-nested-paths",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![23, 20, 25, 20, 25]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("7".to_owned()),
                Some("1".to_owned()),
                Some("red".to_owned()),
                None,
                None,
            ],
            vec![
                Some("7".to_owned()),
                Some("2".to_owned()),
                Some("blue".to_owned()),
                None,
                None,
            ],
            vec![
                Some("7".to_owned()),
                None,
                None,
                Some("1".to_owned()),
                Some("urgent".to_owned()),
            ],
            vec![
                Some("7".to_owned()),
                None,
                None,
                Some("2".to_owned()),
                Some("new".to_owned()),
            ],
        ]
    );
}
