use super::parse_graph_table;
use crate::sql::ast::{
    Expr, GraphEdgeDirection, GraphPathMode, GraphQuantifier, GraphSelector, GraphSelectorKind,
    GraphTable,
};
use crate::sql::SqlErrorKind;

const CONTRACT_QUERY: &str = "SELECT g.person_id, g.friend_id
FROM GRAPH_TABLE (
  social MATCH (a IS person) -[e IS knows WHERE e.active = TRUE]->{1,3} (b IS person)
  WHERE a.id = $1
  COLUMNS (a.id AS person_id, b.id AS friend_id)
) AS g;";

const CONTRACT_PATH_QUERY: &str = "SELECT g.path
FROM GRAPH_TABLE (
  social MATCH p = (a IS person WHERE a.id = $1) -[e IS knows]->{0,2} (b IS person)
  COLUMNS (p AS path)
) AS g;";

fn source_of(query: &str) -> &str {
    &query[query.find("GRAPH_TABLE").unwrap()..]
}

fn table(source: &str) -> GraphTable {
    parse_graph_table(source).unwrap().0
}

fn edge_of(source: &str) -> crate::sql::ast::GraphEdgePattern {
    table(source).path.steps.remove(0).edge
}

fn kind_of(source: &str) -> SqlErrorKind {
    parse_graph_table(source).unwrap_err().kind()
}

fn selector_of(keep: &str) -> GraphSelector {
    let source = format!("GRAPH_TABLE (g MATCH (a) KEEP {keep} COLUMNS (a.id))");
    table(&source).selector.unwrap()
}

#[test]
fn should_parse_the_first_contract_example_verbatim() {
    // Arrange
    let source = source_of(CONTRACT_QUERY);

    // Act
    let (parsed, consumed) = parse_graph_table(source).unwrap();

    // Assert
    assert_eq!(parsed.graph.as_str(), "social");
    assert!(parsed.path_variable.is_none());
    assert!(parsed.selector.is_none());
    assert!(parsed.filter.is_some());
    assert_eq!(parsed.path.start.variable.as_deref(), Some("a"));
    assert_eq!(parsed.path.start.labels, ["person"]);
    let step = &parsed.path.steps[0];
    assert_eq!(step.edge.variable.as_deref(), Some("e"));
    assert_eq!(step.edge.labels, ["knows"]);
    assert!(step.edge.filter.is_some());
    assert_eq!(step.edge.direction, GraphEdgeDirection::Outgoing);
    assert_eq!(
        step.edge.quantifier,
        Some(GraphQuantifier::Range { min: 1, max: 3 })
    );
    assert_eq!(step.vertex.variable.as_deref(), Some("b"));
    let aliases: Vec<_> = parsed.columns.iter().map(|c| c.alias.as_deref()).collect();
    assert_eq!(aliases, [Some("person_id"), Some("friend_id")]);
    assert_eq!(&source[consumed..], " AS g;");
}

#[test]
fn should_parse_the_second_contract_example_verbatim() {
    // Arrange
    let source = source_of(CONTRACT_PATH_QUERY);

    // Act
    let (parsed, consumed) = parse_graph_table(source).unwrap();

    // Assert
    assert_eq!(parsed.path_variable.as_deref(), Some("p"));
    assert!(parsed.path.start.filter.is_some());
    let step = &parsed.path.steps[0];
    assert_eq!(
        step.edge.quantifier,
        Some(GraphQuantifier::Range { min: 0, max: 2 })
    );
    assert!(parsed.filter.is_none());
    assert_eq!(parsed.columns.len(), 1);
    assert!(matches!(&parsed.columns[0].expr, Expr::Column(name) if name == "p"));
    assert_eq!(parsed.columns[0].alias.as_deref(), Some("path"));
    assert_eq!(&source[consumed..], " AS g;");
}

#[test]
fn should_parse_a_single_vertex_path() {
    // Arrange
    let source = "GRAPH_TABLE (social MATCH (a IS person) COLUMNS (a.id))";

    // Act
    let parsed = table(source);

    // Assert
    assert!(parsed.path.steps.is_empty());
    assert_eq!(parsed.path.start.labels, ["person"]);
    assert!(parsed.columns[0].alias.is_none());
}

#[test]
fn should_parse_an_anonymous_vertex() {
    // Arrange
    let source = "GRAPH_TABLE (social MATCH () COLUMNS (1 AS one))";

    // Act
    let parsed = table(source);

    // Assert
    assert!(parsed.path.start.variable.is_none());
    assert_eq!(parsed.path.start.labels.len(), 0);
    assert!(parsed.path.start.filter.is_none());
}

#[test]
fn should_parse_a_vertex_without_variable_but_with_label() {
    // Arrange
    let source = "GRAPH_TABLE (social MATCH (IS person) COLUMNS (1 AS one))";

    // Act
    let parsed = table(source);

    // Assert
    assert!(parsed.path.start.variable.is_none());
    assert_eq!(parsed.path.start.labels, ["person"]);
}

#[test]
fn should_parse_label_alternation() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a IS person | company) COLUMNS (a.id))";

    // Act
    let parsed = table(source);

    // Assert
    assert_eq!(parsed.path.start.labels, ["person", "company"]);
}

#[test]
fn should_parse_an_outgoing_edge() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->(b) COLUMNS (a.id))";

    // Act
    let edge = edge_of(source);

    // Assert
    assert_eq!(edge.direction, GraphEdgeDirection::Outgoing);
    assert!(edge.quantifier.is_none());
}

#[test]
fn should_parse_an_incoming_edge() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)<-[e IS knows]-(b) COLUMNS (a.id))";

    // Act
    let edge = edge_of(source);

    // Assert
    assert_eq!(edge.direction, GraphEdgeDirection::Incoming);
    assert_eq!(edge.labels, ["knows"]);
}

#[test]
fn should_parse_an_undirected_edge() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]-(b) COLUMNS (a.id))";

    // Act
    let edge = edge_of(source);

    // Assert
    assert_eq!(edge.direction, GraphEdgeDirection::Undirected);
}

#[test]
fn should_parse_an_anonymous_edge() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[]->(b) COLUMNS (a.id))";

    // Act
    let edge = edge_of(source);

    // Assert
    assert!(edge.variable.is_none());
    assert_eq!(edge.labels.len(), 0);
    assert!(edge.filter.is_none());
}

#[test]
fn should_parse_an_exact_quantifier() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->{2}(b) COLUMNS (a.id))";

    // Act
    let edge = edge_of(source);

    // Assert
    assert_eq!(edge.quantifier, Some(GraphQuantifier::Exact(2)));
}

#[test]
fn should_parse_a_range_quantifier_with_whitespace() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]-> { 1 , 3 } (b) COLUMNS (a.id))";

    // Act
    let edge = edge_of(source);

    // Assert
    assert_eq!(
        edge.quantifier,
        Some(GraphQuantifier::Range { min: 1, max: 3 })
    );
}

#[test]
fn should_parse_a_star_quantifier() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->*(b) COLUMNS (a.id))";

    // Act
    let edge = edge_of(source);

    // Assert
    assert_eq!(edge.quantifier, Some(GraphQuantifier::ZeroOrMore));
}

#[test]
fn should_parse_a_plus_quantifier() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)<-[e]-+(b) COLUMNS (a.id))";

    // Act
    let edge = edge_of(source);

    // Assert
    assert_eq!(edge.direction, GraphEdgeDirection::Incoming);
    assert_eq!(edge.quantifier, Some(GraphQuantifier::OneOrMore));
}

#[test]
fn should_parse_a_chain_of_mixed_edges() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a IS person)-[x IS knows]->(b IS person)<-[y IS owns]-(c IS company)-[z]-{1,2}(d) COLUMNS (a.id))";

    // Act
    let parsed = table(source);

    // Assert
    let directions: Vec<_> = parsed.path.steps.iter().map(|s| s.edge.direction).collect();
    assert_eq!(
        directions,
        [
            GraphEdgeDirection::Outgoing,
            GraphEdgeDirection::Incoming,
            GraphEdgeDirection::Undirected
        ]
    );
    assert_eq!(parsed.path.steps[1].vertex.labels, ["company"]);
}

#[test]
fn should_parse_an_inline_vertex_predicate_with_nested_parentheses() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a IS person WHERE a.id IN (1, 2)) COLUMNS (a.id))";

    // Act
    let parsed = table(source);

    // Assert
    assert!(parsed.path.start.filter.is_some());
}

#[test]
fn should_parse_an_inline_edge_predicate_containing_brackets() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e WHERE e.tags[1] = 'x']->(b) COLUMNS (a.id))";

    // Act
    let edge = edge_of(source);

    // Assert
    assert!(edge.filter.is_some());
}

#[test]
fn should_parse_the_pattern_where_clause() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->(b) WHERE a.id <> b.id COLUMNS (a.id))";

    // Act
    let parsed = table(source);

    // Assert
    assert!(parsed.filter.is_some());
}

#[test]
fn should_parse_multiple_projections_with_optional_aliases() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->(b) COLUMNS (a.id AS Source, CAST(b.id AS text), b.name AS \"Target\"))";

    // Act
    let parsed = table(source);

    // Assert
    let aliases: Vec<_> = parsed.columns.iter().map(|c| c.alias.as_deref()).collect();
    assert_eq!(aliases, [Some("source"), None, Some("\"Target\"")]);
}

#[test]
fn should_default_to_all_walk_paths_without_keep() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a) COLUMNS (a.id))";

    // Act
    let parsed = table(source);

    // Assert
    assert!(parsed.selector.is_none());
    assert_eq!(parsed.effective_selector().kind, GraphSelectorKind::All);
    assert_eq!(parsed.effective_selector().mode, GraphPathMode::Walk);
}

#[test]
fn should_parse_keep_all_paths() {
    // Arrange
    let keep = "ALL PATHS";

    // Act
    let selector = selector_of(keep);

    // Assert
    assert_eq!(selector.kind, GraphSelectorKind::All);
    assert_eq!(selector.mode, GraphPathMode::Walk);
}

#[test]
fn should_parse_keep_all_trail_paths() {
    // Arrange
    let keep = "all trail paths";

    // Act
    let selector = selector_of(keep);

    // Assert
    assert_eq!(selector.kind, GraphSelectorKind::All);
    assert_eq!(selector.mode, GraphPathMode::Trail);
}

#[test]
fn should_parse_keep_any_shortest_path() {
    // Arrange
    let keep = "ANY SHORTEST PATH";

    // Act
    let selector = selector_of(keep);

    // Assert
    assert_eq!(selector.kind, GraphSelectorKind::AnyShortest);
    assert_eq!(selector.mode, GraphPathMode::Walk);
}

#[test]
fn should_parse_keep_any_shortest_acyclic_path() {
    // Arrange
    let keep = "ANY SHORTEST ACYCLIC PATH";

    // Act
    let selector = selector_of(keep);

    // Assert
    assert_eq!(selector.kind, GraphSelectorKind::AnyShortest);
    assert_eq!(selector.mode, GraphPathMode::Acyclic);
}

#[test]
fn should_parse_keep_shortest_k_paths() {
    // Arrange
    let keep = "SHORTEST 3 PATHS";

    // Act
    let selector = selector_of(keep);

    // Assert
    assert_eq!(selector.kind, GraphSelectorKind::Shortest(3));
}

#[test]
fn should_parse_keep_shortest_k_simple_paths() {
    // Arrange
    let keep = "SHORTEST 2 SIMPLE PATHS";

    // Act
    let selector = selector_of(keep);

    // Assert
    assert_eq!(selector.kind, GraphSelectorKind::Shortest(2));
    assert_eq!(selector.mode, GraphPathMode::Simple);
}

#[test]
fn should_parse_keep_followed_by_pattern_where() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->(b) KEEP ANY SHORTEST PATH WHERE a.id <> b.id COLUMNS (a.id))";

    // Act
    let parsed = table(source);

    // Assert
    assert_eq!(
        parsed.effective_selector().kind,
        GraphSelectorKind::AnyShortest
    );
    assert!(parsed.filter.is_some());
}

#[test]
fn should_fold_unquoted_names_but_keep_quoted_names() {
    // Arrange
    let source =
        "GRAPH_TABLE (Sales.Social MATCH (A IS Person)-[\"E\" IS Knows]->(B) COLUMNS (A.id))";

    // Act
    let parsed = table(source);

    // Assert
    assert_eq!(parsed.graph.as_str(), "sales.social");
    assert_eq!(parsed.path.start.variable.as_deref(), Some("a"));
    assert_eq!(parsed.path.start.labels, ["person"]);
    assert_eq!(parsed.path.steps[0].edge.variable.as_deref(), Some("E"));
}

#[test]
fn should_report_the_consumed_length_after_leading_whitespace() {
    // Arrange
    let source = "  graph_table(g MATCH (a) COLUMNS (a.id)) g2 WHERE true";

    // Act
    let (_, consumed) = parse_graph_table(source).unwrap();

    // Assert
    assert_eq!(&source[consumed..], " g2 WHERE true");
}

#[test]
fn should_reject_comma_separated_patterns_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->(b), (c) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_pattern_unions_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->(b) | (c)-[f]->(d) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_graph_mutation_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a) DELETE a COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_a_question_mark_quantifier_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->?(b) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_an_open_upper_bound_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->{2,}(b) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_an_open_lower_bound_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->{,2}(b) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_nested_quantifiers_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->{1,2}*(b) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_parenthesized_repetition_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->(b){2} COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_a_parameter_quantifier_bound_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->{$1}(b) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_a_bidirectional_edge_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)<-[e]->(b) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_label_conjunction_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a IS person & admin) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_a_parameter_selector_count_as_unsupported() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a) KEEP SHORTEST $1 PATHS COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_a_descending_range_with_invalid_parameter_value() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->{3,1}(b) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::InvalidParameterValue);
}

#[test]
fn should_reject_an_overflowing_bound_with_invalid_parameter_value() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->{99999999999}(b) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::InvalidParameterValue);
}

#[test]
fn should_reject_a_zero_shortest_count_with_invalid_parameter_value() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a) KEEP SHORTEST 0 PATHS COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::InvalidParameterValue);
}

#[test]
fn should_reject_a_missing_columns_clause_as_syntax() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]->(b))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}

#[test]
fn should_reject_a_missing_match_keyword_as_syntax() {
    // Arrange
    let source = "GRAPH_TABLE (g (a) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}

#[test]
fn should_reject_an_unterminated_body_as_syntax() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a) COLUMNS (a.id)";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}

#[test]
fn should_reject_an_abbreviated_edge_as_syntax() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)->(b) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}

#[test]
fn should_reject_an_edge_without_a_closing_dash_as_syntax() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e](b) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}

#[test]
fn should_reject_a_path_ending_in_an_edge_as_syntax() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a)-[e]-> COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}

#[test]
fn should_reject_a_malformed_selector_as_syntax() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a) KEEP ALL PATH COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}

#[test]
fn should_reject_an_empty_inline_predicate_as_syntax() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a WHERE) COLUMNS (a.id))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}

#[test]
fn should_reject_a_projection_alias_without_a_name_as_syntax() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a) COLUMNS (a.id AS ))";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}

#[test]
fn should_reject_text_after_columns_as_syntax() {
    // Arrange
    let source = "GRAPH_TABLE (g MATCH (a) COLUMNS (a.id) extra)";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}

#[test]
fn should_reject_a_source_that_is_not_graph_table_as_syntax() {
    // Arrange
    let source = "people AS p";

    // Act
    let kind = kind_of(source);

    // Assert
    assert_eq!(kind, SqlErrorKind::Syntax);
}
