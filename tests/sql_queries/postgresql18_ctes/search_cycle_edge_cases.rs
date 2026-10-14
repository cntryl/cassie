//! Recursive SEARCH and CYCLE edge cases for PostgreSQL 18 issue #801.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_return_multicolumn_breadth_first_search_sequences() {
    // Arrange
    let fixture = sql_fixture("issue-801-search-breadth-first-multikey", &[]);

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE tree(parent, node, priority) AS (\
             VALUES (NULL::INT4, 1::INT4, 0::INT4), (1, 2, 2), (1, 3, 1),\
                    (2, 4, 1), (3, 5, 2)\
         ), walk(node, priority) AS (\
             SELECT node, priority FROM tree WHERE parent IS NULL\
             UNION ALL\
             SELECT child.node, child.priority\
             FROM tree AS child JOIN walk AS parent ON child.parent = parent.node\
         ) SEARCH BREADTH FIRST BY priority, node SET sequence\
         SELECT node, priority, sequence, sequence::TEXT AS sequence_text\
         FROM walk ORDER BY sequence",
    );

    // Assert
    let result = result.expect("multi-column breadth-first SEARCH sequences");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 2249, 25]
    );
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(0),
                Value::String("(0,0,1)".to_owned()),
                Value::String("(0,0,1)".to_owned()),
            ],
            vec![
                Value::Int64(3),
                Value::Int64(1),
                Value::String("(1,1,3)".to_owned()),
                Value::String("(1,1,3)".to_owned()),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(2),
                Value::String("(1,2,2)".to_owned()),
                Value::String("(1,2,2)".to_owned()),
            ],
            vec![
                Value::Int64(4),
                Value::Int64(1),
                Value::String("(2,1,4)".to_owned()),
                Value::String("(2,1,4)".to_owned()),
            ],
            vec![
                Value::Int64(5),
                Value::Int64(2),
                Value::String("(2,2,5)".to_owned()),
                Value::String("(2,2,5)".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_mark_a_repeated_null_cycle_key() {
    // Arrange
    let fixture = sql_fixture("issue-801-cycle-null-key", &[]);

    // Act
    let result = fixture.execute(
        "WITH RECURSIVE walk(node, depth) AS (\
             SELECT NULL::INT4, 0::INT4\
             UNION ALL\
             SELECT CASE WHEN depth = 0 THEN NULL::INT4 ELSE 1::INT4 END, depth + 1\
             FROM walk WHERE depth < 2\
         ) CYCLE node SET is_cycle USING path\
         SELECT node, depth, is_cycle, path, path::TEXT AS path_text\
         FROM walk ORDER BY depth",
    );

    // Assert
    let result = result.expect("CYCLE must compare repeated NULL keys");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 16, 2287, 25]
    );
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::Null,
                Value::Int64(0),
                Value::Bool(false),
                Value::String("{()}".to_owned()),
                Value::String("{()}".to_owned()),
            ],
            vec![
                Value::Null,
                Value::Int64(1),
                Value::Bool(true),
                Value::String("{(),()}".to_owned()),
                Value::String("{(),()}".to_owned()),
            ],
        ]
    );
}
