//! PostgreSQL 18 CTE modifier and recursive SEARCH/CYCLE grammar tracked by #801.

use cassie::sql::ast::{CteQuery, QueryStatement, SelectItem};

fn parsed_select(sql: &str) -> cassie::sql::ast::SelectStatement {
    let parsed = cassie::sql::parse_statement(sql).expect("CTE statement parses");
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected SELECT statement");
    };
    statement
}

#[test]
fn should_parse_materialized_cte_modifier() {
    // Arrange
    let sql = "WITH chosen AS MATERIALIZED (SELECT 1 AS value) SELECT value FROM chosen";

    // Act
    let statement = parsed_select(sql);

    // Assert
    assert_eq!(statement.ctes.len(), 1);
    assert_eq!(statement.ctes[0].name, "chosen");
    assert!(matches!(statement.ctes[0].query, CteQuery::Simple(_)));
}

#[test]
fn should_parse_not_materialized_cte_modifier() {
    // Arrange
    let sql = "WITH chosen AS NOT MATERIALIZED (SELECT 1 AS value) SELECT value FROM chosen";

    // Act
    let statement = parsed_select(sql);

    // Assert
    assert_eq!(statement.ctes.len(), 1);
    assert_eq!(statement.ctes[0].name, "chosen");
    assert!(matches!(statement.ctes[0].query, CteQuery::Simple(_)));
}

#[test]
fn should_parse_recursive_depth_first_search_clause() {
    // Arrange
    let sql = "WITH RECURSIVE walk(node) AS (SELECT CAST(1 AS BIGINT) UNION ALL SELECT node + 1 FROM walk WHERE node < 2) SEARCH DEPTH FIRST BY node SET sequence SELECT node, sequence FROM walk ORDER BY sequence";

    // Act
    let statement = parsed_select(sql);

    // Assert
    assert!(statement.recursive);
    assert!(matches!(
        statement.ctes[0].query,
        CteQuery::Recursive { .. }
    ));
    assert!(matches!(
        &statement.projection[1],
        SelectItem::Column { name, .. } if name == "sequence"
    ));
}

#[test]
fn should_parse_recursive_breadth_first_search_clause() {
    // Arrange
    let sql = "WITH RECURSIVE walk(node) AS (SELECT CAST(1 AS BIGINT) UNION ALL SELECT node + 1 FROM walk WHERE node < 2) SEARCH BREADTH FIRST BY node SET sequence SELECT node, sequence FROM walk ORDER BY sequence";

    // Act
    let statement = parsed_select(sql);

    // Assert
    assert!(statement.recursive);
    assert!(matches!(
        statement.ctes[0].query,
        CteQuery::Recursive { .. }
    ));
    assert!(matches!(
        &statement.projection[1],
        SelectItem::Column { name, .. } if name == "sequence"
    ));
}

#[test]
fn should_parse_recursive_cycle_clause_with_custom_mark_values() {
    // Arrange
    let sql = "WITH RECURSIVE walk(node) AS (SELECT CAST(1 AS BIGINT) UNION ALL SELECT node + 1 FROM walk WHERE node < 2) CYCLE node SET marker TO 'cycle' DEFAULT 'clear' USING path SELECT node, marker FROM walk";

    // Act
    let statement = parsed_select(sql);

    // Assert
    assert!(statement.recursive);
    assert!(matches!(
        statement.ctes[0].query,
        CteQuery::Recursive { .. }
    ));
    assert!(matches!(
        &statement.projection[1],
        SelectItem::Column { name, .. } if name == "marker"
    ));
}
