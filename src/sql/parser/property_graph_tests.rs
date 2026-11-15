use crate::sql::ast::{
    GraphEndpoint, GraphLabel, GraphProperty, PropertyGraphAction, QueryStatement,
};
use crate::sql::parser::parse_statement;
use crate::sql::SqlErrorKind;

const SOCIAL: &str = "CREATE PROPERTY GRAPH social
  VERTEX TABLES (
    people AS person KEY (id) LABEL person PROPERTIES (id, name)
  )
  EDGE TABLES (
    follows AS knows KEY (id)
      SOURCE KEY (follower_id) REFERENCES person (id)
      DESTINATION KEY (followee_id) REFERENCES person (id)
      LABEL knows PROPERTIES (active, cost AS price)
  )";

fn action(sql: &str) -> PropertyGraphAction {
    match parse_statement(sql).unwrap().statement {
        QueryStatement::PropertyGraph(statement) => statement.action,
        other => panic!("expected a property graph statement, got {other:?}"),
    }
}

fn property(column: &str, name: &str) -> GraphProperty {
    GraphProperty {
        column: column.to_owned(),
        name: name.to_owned(),
    }
}

#[test]
fn should_parse_vertex_mapping_of_the_contract_example() {
    // Arrange
    let sql = SOCIAL;

    // Act
    let PropertyGraphAction::Create(graph) = action(sql) else {
        panic!("expected CREATE");
    };
    let vertex = &graph.vertices[0];

    // Assert
    assert_eq!(graph.name.as_str(), "social");
    assert_eq!(graph.vertices.len(), 1);
    assert_eq!(vertex.table.as_str(), "people");
    assert_eq!(vertex.alias, "person");
    assert_eq!(vertex.key, vec!["id".to_owned()]);
    assert_eq!(
        vertex.labels,
        vec![GraphLabel {
            name: "person".to_owned(),
            properties: vec![property("id", "id"), property("name", "name")],
        }]
    );
}

#[test]
fn should_parse_edge_mapping_of_the_contract_example() {
    // Arrange
    let sql = SOCIAL;

    // Act
    let PropertyGraphAction::Create(graph) = action(sql) else {
        panic!("expected CREATE");
    };
    let edge = &graph.edges[0];

    // Assert
    assert_eq!(edge.table.as_str(), "follows");
    assert_eq!(edge.alias, "knows");
    assert_eq!(edge.key, vec!["id".to_owned()]);
    assert_eq!(
        edge.source,
        GraphEndpoint {
            columns: vec!["follower_id".to_owned()],
            vertex_alias: "person".to_owned(),
            references: vec!["id".to_owned()],
        }
    );
    assert_eq!(edge.destination.columns, vec!["followee_id".to_owned()]);
    assert_eq!(
        edge.labels[0].properties,
        vec![property("active", "active"), property("cost", "price")]
    );
}

#[test]
fn should_parse_composite_keys_with_several_mappings_per_kind() {
    // Arrange
    let sql = "CREATE PROPERTY GRAPH app.org
        VERTEX TABLES (
          app.people AS person KEY (tenant, id) LABEL person PROPERTIES (id) LABEL actor PROPERTIES (id),
          app.teams AS team KEY (tenant, id) LABEL team PROPERTIES (id)
        )
        EDGE TABLES (
          app.members AS member KEY (tenant, id)
            SOURCE KEY (tenant, person_id) REFERENCES person (tenant, id)
            DESTINATION KEY (tenant, team_id) REFERENCES team (tenant, id)
            LABEL member PROPERTIES (role)
        )";

    // Act
    let PropertyGraphAction::Create(graph) = action(sql) else {
        panic!("expected CREATE");
    };

    // Assert
    assert_eq!(graph.name.as_str(), "app.org");
    assert_eq!(graph.vertices.len(), 2);
    assert_eq!(graph.vertices[0].labels.len(), 2);
    assert_eq!(
        graph.vertices[1].key,
        vec!["tenant".to_owned(), "id".to_owned()]
    );
    assert_eq!(
        graph.edges[0].destination.references,
        vec!["tenant".to_owned(), "id".to_owned()]
    );
}

#[test]
fn should_fold_unquoted_names_but_keep_quoted_ones() {
    // Arrange
    let sql = "CREATE PROPERTY GRAPH Social
        VERTEX TABLES (People AS \"Person\" KEY (ID) LABEL Person PROPERTIES (\"Full Name\" AS Name))
        EDGE TABLES (follows AS knows KEY (id)
          SOURCE KEY (a) REFERENCES \"Person\" (ID)
          DESTINATION KEY (b) REFERENCES \"Person\" (ID)
          LABEL knows PROPERTIES (id))";

    // Act
    let PropertyGraphAction::Create(graph) = action(sql) else {
        panic!("expected CREATE");
    };
    let vertex = &graph.vertices[0];

    // Assert
    assert_eq!(graph.name.as_str(), "social");
    assert_eq!(vertex.alias, "Person");
    assert_eq!(vertex.key, vec!["id".to_owned()]);
    assert_eq!(vertex.labels[0].name, "person");
    assert_eq!(
        vertex.labels[0].properties,
        vec![property("Full Name", "name")]
    );
    assert_eq!(graph.edges[0].source.vertex_alias, "Person");
}

#[test]
fn should_parse_drop_with_optional_if_exists() {
    // Arrange
    let statements = [
        "DROP PROPERTY GRAPH social",
        "drop property graph if exists app.social;",
    ];

    // Act
    let [plain, guarded] = statements.map(action);

    // Assert
    assert!(matches!(
        plain,
        PropertyGraphAction::Drop { ref name, if_exists: false } if name.as_str() == "social"
    ));
    assert!(matches!(
        guarded,
        PropertyGraphAction::Drop { ref name, if_exists: true } if name.as_str() == "app.social"
    ));
}

#[test]
fn should_parse_rename() {
    // Arrange
    let sql = "ALTER PROPERTY GRAPH social RENAME TO Network";

    // Act
    let renamed = action(sql);

    // Assert
    assert!(matches!(
        renamed,
        PropertyGraphAction::Rename { ref name, ref new_name }
            if name.as_str() == "social" && new_name == "network"
    ));
}

#[test]
fn should_reject_malformed_definitions_as_syntax_errors() {
    // Arrange
    let malformed = [
        "CREATE PROPERTY GRAPH social",
        "CREATE PROPERTY GRAPH social VERTEX TABLES () EDGE TABLES ()",
        "CREATE PROPERTY GRAPH social VERTEX TABLES (people person KEY (id) LABEL p PROPERTIES (id)) EDGE TABLES (f AS k KEY (id) SOURCE KEY (a) REFERENCES person (id) DESTINATION KEY (b) REFERENCES person (id) LABEL k PROPERTIES (id))",
        "CREATE PROPERTY GRAPH social VERTEX TABLES (people AS person KEY (id)) EDGE TABLES (f AS k KEY (id) SOURCE KEY (a) REFERENCES person (id) DESTINATION KEY (b) REFERENCES person (id) LABEL k PROPERTIES (id))",
        "CREATE PROPERTY GRAPH social VERTEX TABLES (people AS person KEY (id) LABEL p PROPERTIES (id)) EDGE TABLES (f AS k KEY (id) SOURCE KEY (a) REFERENCES person (id) LABEL k PROPERTIES (id))",
        "CREATE PROPERTY GRAPH social VERTEX TABLES (people AS person KEY (id) LABEL p PROPERTIES (id)) EDGE TABLES (f AS k KEY (id) SOURCE KEY (a) REFERENCES person (id) DESTINATION KEY (b) REFERENCES person (id) LABEL k PROPERTIES (id)) extra",
        "ALTER PROPERTY GRAPH social RENAME TO app.network",
        "DROP PROPERTY GRAPH",
    ];

    // Act
    let kinds = malformed.map(|sql| parse_statement(sql).unwrap_err().kind());

    // Assert
    assert_eq!(kinds, [SqlErrorKind::Syntax; 8]);
}

#[test]
fn should_reject_unsupported_graph_alterations() {
    // Arrange
    let unsupported = [
        "DROP PROPERTY GRAPH social CASCADE",
        "ALTER PROPERTY GRAPH social SET SCHEMA app",
        "ALTER PROPERTY GRAPH social ADD VERTEX TABLES (people AS person KEY (id) LABEL p PROPERTIES (id))",
    ];

    // Act
    let kinds = unsupported.map(|sql| parse_statement(sql).unwrap_err().kind());

    // Assert
    assert_eq!(kinds, [SqlErrorKind::Unsupported; 3]);
}
