//! Discovery laws of mapped property graphs (#1255).

use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::types::Value;

const SETUP: [&str; 3] = [
    "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT)",
    "CREATE TABLE follows (a BIGINT NOT NULL, b BIGINT NOT NULL, since TEXT, \
     CONSTRAINT follows_pk PRIMARY KEY (a, b))",
    "CREATE PROPERTY GRAPH social
       VERTEX TABLES (people AS person KEY (id)
         LABEL person PROPERTIES (id, name)
         LABEL named PROPERTIES (name))
       EDGE TABLES (follows AS knows KEY (a, b)
         SOURCE KEY (a) REFERENCES person (id)
         DESTINATION KEY (b) REFERENCES person (id)
         LABEL knows PROPERTIES (since AS started))",
];

fn fixture() -> (Cassie, CassieSession) {
    let password = uuid::Uuid::new_v4().to_string();
    let config = CassieRuntimeConfig {
        password: password.clone(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_in_memory_with_config(config).expect("memory fixture");
    cassie.startup().expect("startup");
    let root = cassie
        .authenticate_role("root", Some(&password), None)
        .expect("root login");
    for sql in SETUP {
        cassie.execute_sql(&root, sql, vec![]).expect("setup");
    }
    (cassie, root)
}

/// The rows of `sql`, each rendered as its values joined by `|`.
fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<String> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("query")
        .rows()
        .iter()
        .map(|row| {
            row.iter()
                .map(|value| match value {
                    Value::String(text) => text.clone(),
                    Value::Int64(number) => number.to_string(),
                    Value::Null => "NULL".to_owned(),
                    other => format!("{other:?}"),
                })
                .collect::<Vec<_>>()
                .join("|")
        })
        .collect()
}

#[test]
fn should_list_graphs_with_their_element_tables() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let graphs = rows(
        &cassie,
        &root,
        "SELECT property_graph_catalog, property_graph_schema, property_graph_name \
         FROM information_schema.property_graphs",
    );
    let elements = rows(
        &cassie,
        &root,
        "SELECT element_table_alias, element_table_kind, table_schema, table_name \
         FROM information_schema.pg_element_tables ORDER BY element_table_kind DESC",
    );

    // Assert
    assert_eq!(graphs, vec!["postgres|public|social"]);
    assert_eq!(
        elements,
        vec!["person|VERTEX|public|people", "knows|EDGE|public|follows"]
    );
}

#[test]
fn should_list_ordered_key_columns_with_edge_components() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let keys = rows(
        &cassie,
        &root,
        "SELECT element_table_alias, column_name, ordinal_position \
         FROM information_schema.pg_element_table_key_columns \
         ORDER BY element_table_alias, ordinal_position",
    );
    let components = rows(
        &cassie,
        &root,
        "SELECT element_table_alias, edge_end, edge_table_column_name, vertex_table_alias, \
         vertex_table_column_name, ordinal_position \
         FROM information_schema.pg_edge_table_components ORDER BY edge_end DESC",
    );

    // Assert
    assert_eq!(keys, vec!["knows|a|1", "knows|b|2", "person|id|1"]);
    assert_eq!(
        components,
        vec![
            "knows|SOURCE|a|person|id|1",
            "knows|DESTINATION|b|person|id|1"
        ]
    );
}

#[test]
fn should_list_labels_with_their_properties() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let labels = rows(
        &cassie,
        &root,
        "SELECT element_table_alias, label_name \
         FROM information_schema.pg_element_table_labels ORDER BY label_name",
    );
    let element_properties = rows(
        &cassie,
        &root,
        "SELECT element_table_alias, property_name, property_expression \
         FROM information_schema.pg_element_table_properties \
         ORDER BY element_table_alias, property_name",
    );
    let label_properties = rows(
        &cassie,
        &root,
        "SELECT label_name, property_name FROM information_schema.pg_label_properties \
         ORDER BY label_name, property_name",
    );

    // Assert
    assert_eq!(labels, vec!["knows|knows", "person|named", "person|person"]);
    assert_eq!(
        element_properties,
        vec!["knows|started|since", "person|id|id", "person|name|name"]
    );
    assert_eq!(
        label_properties,
        vec!["knows|started", "named|name", "person|id", "person|name"]
    );
}

#[test]
fn should_expose_the_durable_identities_of_a_graph() {
    // Arrange
    let (cassie, root) = fixture();
    let stored = cassie
        .catalog
        .get_property_graph(&cassie.midge.canonical_collection_name("social"))
        .expect("graph");

    // Act
    let identities = rows(
        &cassie,
        &root,
        "SELECT property_graph_name, property_graph_oid, property_graph_id, database_id, \
         element_table_alias, element_table_kind, element_table_id \
         FROM pg_catalog.pg_property_graph_identities ORDER BY element_table_kind DESC",
    );

    // Assert
    let graph = format!(
        "social|{}|{}|{}",
        stored.object_id, stored.graph_id, stored.database_id
    );
    assert_eq!(
        identities,
        vec![
            format!("{graph}|person|VERTEX|{}", stored.vertices[0].mapping_id),
            format!("{graph}|knows|EDGE|{}", stored.edges[0].mapping_id),
        ]
    );
}

#[test]
fn should_hide_a_graph_from_a_role_that_cannot_read_every_mapped_table() {
    // Arrange
    let (cassie, root) = fixture();
    for sql in [
        "CREATE ROLE reader LOGIN PASSWORD 'reader-secret'",
        "GRANT USAGE ON SCHEMA public TO reader",
        "GRANT SELECT ON TABLE public.people TO reader",
    ] {
        cassie.execute_sql(&root, sql, vec![]).expect("grant");
    }
    let reader = cassie
        .authenticate_role("reader", Some("reader-secret"), None)
        .expect("reader login");
    let views = [
        "information_schema.property_graphs",
        "information_schema.pg_element_tables",
        "information_schema.pg_element_table_key_columns",
        "information_schema.pg_edge_table_components",
        "information_schema.pg_element_table_labels",
        "information_schema.pg_element_table_properties",
        "information_schema.pg_label_properties",
        "pg_catalog.pg_property_graph_identities",
    ];
    let count = |session: &CassieSession| {
        views
            .iter()
            .map(|view| rows(&cassie, session, &format!("SELECT * FROM {view}")).len())
            .collect::<Vec<_>>()
    };

    for sql in [
        "CREATE ROLE outsider LOGIN PASSWORD 'outsider-secret'",
        "GRANT SELECT ON TABLE public.people TO outsider",
        "GRANT SELECT ON TABLE public.follows TO outsider",
    ] {
        cassie.execute_sql(&root, sql, vec![]).expect("grant");
    }
    let outsider = cassie
        .authenticate_role("outsider", Some("outsider-secret"), None)
        .expect("outsider login");

    // Act
    let without_schema_usage = count(&outsider);
    let partial = count(&reader);
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.follows TO reader",
            vec![],
        )
        .expect("grant");
    let full = count(&reader);
    let admin = count(&root);

    // Assert
    assert_eq!(without_schema_usage, vec![0; 8]);
    assert_eq!(partial, vec![0; 8]);
    assert_eq!(full, admin);
    assert_eq!(admin, vec![1, 2, 3, 2, 3, 3, 4, 2]);
}

#[test]
fn should_refuse_graph_definition_to_a_role_that_is_not_an_administrator() {
    // Arrange
    let (cassie, root) = fixture();
    for sql in [
        "CREATE ROLE reader LOGIN PASSWORD 'reader-secret'",
        "GRANT USAGE ON SCHEMA public TO reader",
        "GRANT SELECT ON TABLE public.people TO reader",
        "GRANT SELECT ON TABLE public.follows TO reader",
    ] {
        cassie.execute_sql(&root, sql, vec![]).expect("grant");
    }
    let reader = cassie
        .authenticate_role("reader", Some("reader-secret"), None)
        .expect("reader login");

    // Act
    let states = [
        "DROP PROPERTY GRAPH social",
        "ALTER PROPERTY GRAPH social RENAME TO mine",
        "CREATE PROPERTY GRAPH mine
           VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id))
           EDGE TABLES (follows AS knows KEY (a, b)
             SOURCE KEY (a) REFERENCES person (id)
             DESTINATION KEY (b) REFERENCES person (id)
             LABEL knows PROPERTIES (since))",
    ]
    .map(|sql| {
        cassie
            .execute_sql(&reader, sql, vec![])
            .map(|_| ())
            .expect_err("refused")
            .sql_state()
    });

    // Assert
    assert_eq!(states, ["42501"; 3]);
    assert_eq!(cassie.catalog.list_property_graphs().len(), 1);
}
