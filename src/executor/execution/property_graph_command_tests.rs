//! Property graph DDL laws: definition validation, drop and rename (#1255).

use crate::app::{Cassie, CassieError, CassieSession};
use crate::config::CassieRuntimeConfig;

const PEOPLE: &str = "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT, nick TEXT)";
const FOLLOWS: &str = "CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, \
                       followee_id BIGINT NOT NULL, tag TEXT, cost INT)";
const SOCIAL: &str = "CREATE PROPERTY GRAPH social
  VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id, name))
  EDGE TABLES (
    follows AS knows KEY (id)
      SOURCE KEY (follower_id) REFERENCES person (id)
      DESTINATION KEY (followee_id) REFERENCES person (id)
      LABEL knows PROPERTIES (cost AS price))";

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [PEOPLE, FOLLOWS] {
        cassie.execute_sql(&session, sql, vec![]).expect("table");
    }
    (cassie, session)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> Result<String, CassieError> {
    cassie
        .execute_sql(session, sql, vec![])
        .map(|result| result.command().to_owned())
}

fn state(cassie: &Cassie, session: &CassieSession, sql: &str) -> &'static str {
    run(cassie, session, sql)
        .expect_err("statement should fail")
        .sql_state()
}

/// An edge clause that is valid over `people AS v KEY (id)`.
const EDGES: &str = "EDGE TABLES (follows AS e KEY (id)
    SOURCE KEY (follower_id) REFERENCES v (id)
    DESTINATION KEY (followee_id) REFERENCES v (id)
    LABEL e PROPERTIES (cost))";

fn graph(cassie: &Cassie, name: &str) -> Option<crate::catalog::PropertyGraphMeta> {
    cassie
        .catalog
        .get_property_graph(&cassie.midge.canonical_collection_name(name))
}

/// A one-vertex graph; an empty `labels` uses a label exposing no checked column.
fn vertex_graph(table: &str, key: &str, labels: &str) -> String {
    let tail = if labels.is_empty() {
        "LABEL v PROPERTIES (id)"
    } else {
        labels
    };
    format!("CREATE PROPERTY GRAPH g VERTEX TABLES ({table} AS v KEY ({key}) {tail}) {EDGES}")
}

fn edge_graph(source: &str, destination: &str) -> String {
    format!(
        "CREATE PROPERTY GRAPH g
           VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id))
           EDGE TABLES (follows AS knows KEY (id)
             SOURCE KEY {source} DESTINATION KEY {destination}
             LABEL knows PROPERTIES (cost))"
    )
}

#[test]
fn should_store_the_contract_example_with_typed_mappings() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let command = run(&cassie, &session, SOCIAL).expect("create");
    let graph = graph(&cassie, "social").expect("stored");

    // Assert
    assert_eq!(command, "CREATE PROPERTY GRAPH");
    assert_eq!(graph.vertices.len(), 1);
    assert_eq!(graph.vertices[0].alias, "person");
    assert_eq!(graph.vertices[0].key[0].name, "id");
    assert_eq!(graph.vertices[0].labels[0].properties.len(), 2);
    assert_eq!(
        graph.edges[0].source.columns,
        vec!["follower_id".to_owned()]
    );
    assert_eq!(graph.edges[0].destination.vertex_alias, "person");
    assert_eq!(graph.edges[0].labels[0].properties[0].name, "price");
    assert_eq!(graph.edges[0].labels[0].properties[0].column, "cost");
}

#[test]
fn should_mint_distinct_durable_identities_for_graph_parts() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    run(&cassie, &session, SOCIAL).expect("create");
    let graph = graph(&cassie, "social").expect("stored");
    let mut ids = vec![
        graph.graph_id.clone(),
        graph.database_id.clone(),
        graph.vertices[0].mapping_id.clone(),
        graph.edges[0].mapping_id.clone(),
    ];
    let parsed = ids
        .iter()
        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
        .count();
    ids.sort_unstable();
    ids.dedup();

    // Assert
    assert_eq!(parsed, 4);
    assert_eq!(ids.len(), 4);
    assert_ne!(graph.object_id, 0);
}

#[test]
fn should_reject_a_second_graph_with_the_same_name() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, SOCIAL).expect("create");

    // Act
    let again = state(&cassie, &session, SOCIAL);

    // Assert
    assert_eq!(again, "42710");
}

#[test]
fn should_reject_missing_schema_table_or_column() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let schema = state(
        &cassie,
        &session,
        &format!(
            "CREATE PROPERTY GRAPH nowhere.g VERTEX TABLES (people AS v KEY (id) LABEL v PROPERTIES (id)) {EDGES}"
        ),
    );
    let table = state(&cassie, &session, &vertex_graph("ghosts", "id", ""));
    let key = state(&cassie, &session, &vertex_graph("people", "missing", ""));
    let property = state(
        &cassie,
        &session,
        &vertex_graph("people", "id", "LABEL p PROPERTIES (missing)"),
    );

    // Assert
    assert_eq!(schema, "3F000");
    assert_eq!(table, "42P01");
    assert_eq!(key, "42703");
    assert_eq!(property, "42703");
    assert!(graph(&cassie, "g").is_none());
}

#[test]
fn should_reject_a_key_without_a_matching_unique_key() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        "CREATE TABLE loose (id BIGINT NOT NULL, code TEXT)",
    )
    .expect("table");
    run(
        &cassie,
        &session,
        "CREATE TABLE nullable_key (id BIGINT UNIQUE)",
    )
    .expect("table");

    // Act
    let not_unique = state(&cassie, &session, &vertex_graph("loose", "id", ""));
    let nullable = state(&cassie, &session, &vertex_graph("nullable_key", "id", ""));
    let wider = state(&cassie, &session, &vertex_graph("people", "id, name", ""));

    // Assert
    assert_eq!(not_unique, "42830");
    assert_eq!(nullable, "42830");
    assert_eq!(wider, "42830");
}

#[test]
fn should_accept_unique_constraint_or_unique_index_keys() {
    // Arrange
    let (cassie, session) = fixture();
    for sql in [
        "CREATE TABLE by_constraint (code TEXT NOT NULL UNIQUE)",
        "CREATE TABLE by_pair (a INT NOT NULL, b INT NOT NULL, UNIQUE (a, b))",
        "CREATE TABLE by_index (ref UUID NOT NULL)",
        "CREATE UNIQUE INDEX by_index_ref ON by_index (ref)",
        "CREATE TABLE links (id BIGINT PRIMARY KEY, a INT NOT NULL, b INT NOT NULL, \
         code TEXT NOT NULL, ref UUID NOT NULL)",
    ] {
        run(&cassie, &session, sql).expect("setup");
    }

    // Act
    let created = run(
        &cassie,
        &session,
        "CREATE PROPERTY GRAPH keyed VERTEX TABLES (
           by_constraint AS c KEY (code) LABEL c PROPERTIES (code),
           by_pair AS p KEY (a, b) LABEL p PROPERTIES (a, b),
           by_index AS i KEY (ref) LABEL i PROPERTIES (ref))
         EDGE TABLES (
           links AS pair_to_code KEY (id)
             SOURCE KEY (a, b) REFERENCES p (a, b)
             DESTINATION KEY (code) REFERENCES c (code)
             LABEL link PROPERTIES (id),
           links AS code_to_ref KEY (id)
             SOURCE KEY (code) REFERENCES c (code)
             DESTINATION KEY (ref) REFERENCES i (ref)
             LABEL link PROPERTIES (id))",
    );
    let reordered = state(&cassie, &session, &vertex_graph("by_pair", "b, a", ""));

    // Assert
    assert!(created.is_ok());
    assert_eq!(reordered, "42830");
}

#[test]
fn should_reject_a_partial_unique_index_as_key() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        "CREATE TABLE partial (id BIGINT NOT NULL, live BOOLEAN)",
    )
    .expect("table");
    run(
        &cassie,
        &session,
        "CREATE UNIQUE INDEX partial_live ON partial (id) WHERE live",
    )
    .expect("index");

    // Act
    let partial = state(&cassie, &session, &vertex_graph("partial", "id", ""));

    // Assert
    assert_eq!(partial, "42830");
}

#[test]
fn should_reject_unsupported_key_types_or_view_sources() {
    // Arrange
    let (cassie, session) = fixture();
    run(
        &cassie,
        &session,
        "CREATE TABLE flags (id BOOLEAN PRIMARY KEY)",
    )
    .expect("table");
    run(
        &cassie,
        &session,
        "CREATE VIEW people_view AS SELECT id, name FROM people",
    )
    .expect("view");

    // Act
    for sql in [
        "CREATE MATERIALIZED VIEW people_copy AS SELECT id, name FROM people",
        "CREATE DOMAIN person_id AS BIGINT",
        "CREATE TABLE by_domain (id person_id PRIMARY KEY)",
        "CREATE TABLE by_float (id DOUBLE PRECISION PRIMARY KEY)",
    ] {
        run(&cassie, &session, sql).expect("setup");
    }
    let key_type = state(&cassie, &session, &vertex_graph("flags", "id", ""));
    let view = state(&cassie, &session, &vertex_graph("people_view", "id", ""));
    let materialized = state(&cassie, &session, &vertex_graph("people_copy", "id", ""));
    let domain = state(&cassie, &session, &vertex_graph("by_domain", "id", ""));
    let float = state(&cassie, &session, &vertex_graph("by_float", "id", ""));

    // Assert
    assert_eq!(key_type, "0A000");
    assert_eq!(view, "0A000");
    assert_eq!(materialized, "0A000");
    assert_eq!(domain, "0A000");
    assert_eq!(float, "0A000");
}

#[test]
fn should_reject_duplicate_aliases_labels_or_properties() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let alias = state(
        &cassie,
        &session,
        &format!(
            "CREATE PROPERTY GRAPH g VERTEX TABLES (
               people AS v KEY (id) LABEL a PROPERTIES (id),
               people AS v KEY (id) LABEL b PROPERTIES (id)) {EDGES}"
        ),
    );
    let label = state(
        &cassie,
        &session,
        &vertex_graph(
            "people",
            "id",
            "LABEL p PROPERTIES (id) LABEL p PROPERTIES (name)",
        ),
    );
    let property = state(
        &cassie,
        &session,
        &vertex_graph("people", "id", "LABEL p PROPERTIES (name, nick AS name)"),
    );

    // Assert
    assert_eq!(alias, "42710");
    assert_eq!(label, "42710");
    assert_eq!(property, "42710");
}

#[test]
fn should_reject_endpoints_that_do_not_match_the_vertex_key() {
    // Arrange
    let (cassie, session) = fixture();
    let good = "(followee_id) REFERENCES person (id)";

    // Act
    let unknown_vertex = state(
        &cassie,
        &session,
        &edge_graph("(follower_id) REFERENCES stranger (id)", good),
    );
    let not_the_key = state(
        &cassie,
        &session,
        &edge_graph("(follower_id) REFERENCES person (name)", good),
    );
    let arity = state(
        &cassie,
        &session,
        &edge_graph("(follower_id, followee_id) REFERENCES person (id)", good),
    );
    let type_mismatch = state(
        &cassie,
        &session,
        &edge_graph("(tag) REFERENCES person (id)", good),
    );
    let narrower = state(
        &cassie,
        &session,
        &edge_graph("(cost) REFERENCES person (id)", good),
    );

    // Assert
    assert_eq!(unknown_vertex, "42704");
    assert_eq!(not_the_key, "42830");
    assert_eq!(arity, "42830");
    assert_eq!(type_mismatch, "42804");
    assert_eq!(narrower, "42804");
}

#[test]
fn should_drop_a_graph_without_touching_its_tables() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, "INSERT INTO people VALUES (1, 'a', 'x')").expect("row");
    run(&cassie, &session, SOCIAL).expect("create");

    // Act
    let command = run(&cassie, &session, "DROP PROPERTY GRAPH social").expect("drop");
    let rows = cassie
        .execute_sql(&session, "SELECT id FROM people", vec![])
        .expect("select")
        .rows()
        .len();

    // Assert
    assert_eq!(command, "DROP PROPERTY GRAPH");
    assert!(graph(&cassie, "social").is_none());
    assert!(cassie
        .midge
        .list_property_graphs()
        .expect("list")
        .is_empty());
    assert_eq!(rows, 1);
}

#[test]
fn should_report_a_missing_graph_unless_if_exists_is_given() {
    // Arrange
    let (cassie, session) = fixture();

    // Act
    let missing = state(&cassie, &session, "DROP PROPERTY GRAPH social");
    let tolerated = run(&cassie, &session, "DROP PROPERTY GRAPH IF EXISTS social");
    let rename = state(
        &cassie,
        &session,
        "ALTER PROPERTY GRAPH social RENAME TO other",
    );

    // Assert
    assert_eq!(missing, "42704");
    assert!(tolerated.is_ok());
    assert_eq!(rename, "42704");
}

#[test]
fn should_keep_every_identity_when_a_graph_is_renamed() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, SOCIAL).expect("create");
    let before = graph(&cassie, "social").expect("stored");

    // Act
    run(
        &cassie,
        &session,
        "ALTER PROPERTY GRAPH social RENAME TO network",
    )
    .expect("rename");
    let after = graph(&cassie, "network").expect("renamed");
    let stored = cassie.midge.list_property_graphs().expect("list");

    // Assert
    assert!(graph(&cassie, "social").is_none());
    assert_eq!(after.graph_id, before.graph_id);
    assert_eq!(after.object_id, before.object_id);
    assert_eq!(after.vertices, before.vertices);
    assert_eq!(after.edges, before.edges);
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].name, after.name);
}

#[test]
fn should_reject_a_rename_onto_an_existing_graph() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, SOCIAL).expect("create");
    run(
        &cassie,
        &session,
        &format!(
            "CREATE PROPERTY GRAPH other VERTEX TABLES (people AS v KEY (id) LABEL v PROPERTIES (id)) {EDGES}"
        ),
    )
    .expect("second");

    // Act
    let taken = state(
        &cassie,
        &session,
        "ALTER PROPERTY GRAPH social RENAME TO other",
    );

    // Assert
    assert_eq!(taken, "42710");
    assert!(graph(&cassie, "social").is_some());
}

#[test]
fn should_mint_new_identities_when_a_graph_is_recreated() {
    // Arrange
    let (cassie, session) = fixture();
    run(&cassie, &session, SOCIAL).expect("create");
    let first = graph(&cassie, "social").expect("stored");
    run(&cassie, &session, "DROP PROPERTY GRAPH social").expect("drop");

    // Act
    run(&cassie, &session, SOCIAL).expect("recreate");
    let second = graph(&cassie, "social").expect("stored");

    // Assert
    assert_ne!(second.graph_id, first.graph_id);
    assert_ne!(second.vertices[0].mapping_id, first.vertices[0].mapping_id);
    assert_eq!(second.database_id, first.database_id);
}

#[test]
fn should_map_row_authoritative_together_with_column_authoritative_tables() {
    // Arrange
    let (cassie, session) = fixture();
    for sql in [
        "CREATE TABLE accounts (id UUID PRIMARY KEY, owner TEXT) USING columnar",
        "CREATE TABLE transfers (id BIGINT PRIMARY KEY, payer UUID NOT NULL, \
         payee UUID NOT NULL, amount BIGINT) USING columnar",
        "CREATE TABLE owners (id BIGINT PRIMARY KEY, account UUID NOT NULL)",
    ] {
        run(&cassie, &session, sql).expect("table");
    }

    // Act
    let created = run(
        &cassie,
        &session,
        "CREATE PROPERTY GRAPH ledger
           VERTEX TABLES (
             accounts AS account KEY (id) LABEL account PROPERTIES (owner),
             people AS person KEY (id) LABEL person PROPERTIES (name))
           EDGE TABLES (
             transfers AS transfer KEY (id)
               SOURCE KEY (payer) REFERENCES account (id)
               DESTINATION KEY (payee) REFERENCES account (id)
               LABEL transfer PROPERTIES (amount),
             owners AS owns KEY (id)
               SOURCE KEY (id) REFERENCES person (id)
               DESTINATION KEY (account) REFERENCES account (id)
               LABEL owns PROPERTIES (id))",
    );
    let stored = graph(&cassie, "ledger").expect("stored");
    let key_types = stored
        .vertices
        .iter()
        .map(|vertex| vertex.key[0].data_type.type_name())
        .collect::<Vec<_>>();

    // Assert
    assert!(created.is_ok());
    assert_eq!(key_types, vec!["uuid".to_owned(), "bigint".to_owned()]);
    assert_eq!(stored.edges.len(), 2);
}
