use crate::ast::{JoinKind, QuerySource, QueryStatement, SelectStatement, TableFunctionForm};
use crate::parser::parse_statement;
use crate::SqlErrorKind;

const BODY: &str = "social MATCH (a IS person WHERE a.id = $1) -[e IS knows]-> (b IS person) \
                    COLUMNS (a.id AS person_id, b.id AS friend_id)";

fn select_of(sql: &str) -> SelectStatement {
    match parse_statement(sql).expect("statement parses").statement {
        QueryStatement::Select(select) => *select,
        _ => panic!("expected a SELECT statement"),
    }
}

/// The aliased `GRAPH_TABLE` source and its alias.
fn aliased(source: &QuerySource) -> (&QuerySource, &str, &[String]) {
    match source {
        QuerySource::Aliased {
            source,
            alias,
            column_aliases,
        } => (source, alias, column_aliases),
        _ => panic!("expected an aliased source"),
    }
}

fn is_graph_table(source: &QuerySource) -> bool {
    matches!(
        source,
        QuerySource::TableFunction { form: Some(form), .. }
            if matches!(form.as_ref(), TableFunctionForm::GraphTable(_))
    )
}

#[test]
fn should_parse_a_graph_table_with_an_alias() {
    // Arrange
    let sql = format!("SELECT g.person_id FROM GRAPH_TABLE ({BODY}) AS g");

    // Act
    let select = select_of(&sql);

    // Assert
    let (inner, alias, columns) = aliased(&select.source);
    assert!(is_graph_table(inner));
    assert_eq!(alias, "g");
    assert_eq!(columns, ["person_id", "friend_id"]);
}

#[test]
fn should_parse_a_graph_table_without_the_as_keyword() {
    // Arrange
    let sql = format!("SELECT g.person_id FROM GRAPH_TABLE ({BODY}) g");

    // Act
    let select = select_of(&sql);

    // Assert
    let (inner, alias, _) = aliased(&select.source);
    assert!(is_graph_table(inner));
    assert_eq!(alias, "g");
}

#[test]
fn should_parse_a_column_alias_list_after_the_alias() {
    // Arrange
    let sql = format!("SELECT g.x FROM GRAPH_TABLE ({BODY}) AS g (x, y)");

    // Act
    let select = select_of(&sql);

    // Assert
    let (_, alias, columns) = aliased(&select.source);
    assert_eq!(alias, "g");
    assert_eq!(columns, ["x", "y"]);
}

#[test]
fn should_name_a_graph_table_without_an_alias_after_its_function() {
    // Arrange
    let sql = format!("SELECT person_id FROM GRAPH_TABLE ({BODY})");

    // Act
    let select = select_of(&sql);

    // Assert
    let (inner, alias, _) = aliased(&select.source);
    assert!(is_graph_table(inner));
    assert_eq!(alias, "graph_table");
}

#[test]
fn should_name_a_bare_property_reference_after_the_property() {
    // Arrange
    let sql = "SELECT g.id FROM GRAPH_TABLE (social MATCH (a IS person) COLUMNS (a.id)) AS g";

    // Act
    let select = select_of(sql);

    // Assert
    let (_, _, columns) = aliased(&select.source);
    assert_eq!(columns, ["id"]);
}

#[test]
fn should_reject_an_unaliased_expression_projection() {
    // Arrange
    let sql = "SELECT 1 FROM GRAPH_TABLE (social MATCH (a IS person) COLUMNS (a.id + 1)) AS g";

    // Act
    let error = parse_statement(sql).expect_err("an expression needs an alias");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Syntax);
}

#[test]
fn should_parse_a_graph_table_as_the_right_side_of_a_join() {
    // Arrange
    let sql = format!(
        "SELECT p.id FROM people AS p JOIN GRAPH_TABLE ({BODY}) AS g ON g.person_id = p.id"
    );

    // Act
    let select = select_of(&sql);

    // Assert
    let QuerySource::Join {
        left, right, kind, ..
    } = &select.source
    else {
        panic!("expected a join");
    };
    assert!(matches!(left.as_ref(), QuerySource::Aliased { .. }));
    assert!(is_graph_table(aliased(right).0));
    assert!(matches!(kind, JoinKind::Inner));
}

#[test]
fn should_mark_a_lateral_graph_table() {
    // Arrange
    let sql = format!("SELECT p.id FROM people AS p, LATERAL GRAPH_TABLE ({BODY}) AS g");

    // Act
    let select = select_of(&sql);

    // Assert
    let QuerySource::Join { right, .. } = &select.source else {
        panic!("expected a join");
    };
    assert!(matches!(
        aliased(right).0,
        QuerySource::TableFunction { lateral: true, .. }
    ));
}

#[test]
fn should_leave_a_graph_table_without_lateral_unmarked() {
    // Arrange
    let sql = format!("SELECT p.id FROM people AS p, GRAPH_TABLE ({BODY}) AS g");

    // Act
    let select = select_of(&sql);

    // Assert
    let QuerySource::Join { right, .. } = &select.source else {
        panic!("expected a join");
    };
    assert!(matches!(
        aliased(right).0,
        QuerySource::TableFunction { lateral: false, .. }
    ));
}

#[test]
fn should_count_parameters_inside_a_graph_table() {
    // Arrange
    let parsed = parse_statement(&format!(
        "SELECT g.person_id FROM GRAPH_TABLE ({BODY}) AS g WHERE g.friend_id = $2"
    ))
    .expect("statement parses");

    // Act
    let count = crate::sql::parameter_count(&parsed);

    // Assert
    assert_eq!(count, 2);
}

#[test]
fn should_count_a_parameter_that_only_a_projection_uses() {
    // Arrange
    let parsed = parse_statement(
        "SELECT g.x FROM GRAPH_TABLE (social MATCH (a IS person) COLUMNS ($3 AS x)) AS g",
    )
    .expect("statement parses");

    // Act
    let count = crate::sql::parameter_count(&parsed);

    // Assert
    assert_eq!(count, 3);
}
