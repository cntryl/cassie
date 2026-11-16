//! Binding of `GRAPH_TABLE` sources against mapped property graphs (#1256).

use super::graph_table_scope::select_elements;
use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;
use crate::sql::ast::GraphVariableKind;
use crate::types::Value;

const SETUP: [&str; 5] = [
    "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT, age INT)",
    "CREATE TABLE companies (id BIGINT PRIMARY KEY, name TEXT, age TEXT)",
    "CREATE TABLE follows (a BIGINT NOT NULL, b BIGINT NOT NULL, since TEXT, active BOOLEAN, \
     CONSTRAINT follows_pk PRIMARY KEY (a, b))",
    "CREATE PROPERTY GRAPH social
       VERTEX TABLES (
         people AS person KEY (id)
           LABEL person PROPERTIES (id, name, age)
           LABEL entity PROPERTIES (id, name),
         companies AS company KEY (id)
           LABEL company PROPERTIES (id, name, age)
           LABEL entity PROPERTIES (id, name))
       EDGE TABLES (follows AS knows KEY (a, b)
         SOURCE KEY (a) REFERENCES person (id)
         DESTINATION KEY (b) REFERENCES person (id)
         LABEL knows PROPERTIES (since, active))",
    "INSERT INTO people VALUES (1, 'ann', 30)",
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

/// The SQLSTATE and message of a failed statement.
fn failure(cassie: &Cassie, session: &CassieSession, sql: &str) -> (&'static str, String) {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(_) => panic!("statement unexpectedly succeeded"),
        Err(error) => (error.sql_state(), error.to_string()),
    }
}

/// The rows of a statement that must bind and execute.
fn succeed(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("statement failed: {error}"),
    }
}

/// Asserts that `sql` binds and executes.
fn assert_binds(cassie: &Cassie, session: &CassieSession, sql: &str) {
    succeed(cassie, session, sql);
}

fn graph_from(table: &str) -> String {
    format!("SELECT * FROM GRAPH_TABLE ({table}) AS g")
}

// Constructs that bind.

#[test]
fn should_bind_a_single_vertex_pattern() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let sql = graph_from("social MATCH (a IS person) COLUMNS (a.id AS pid, a.name AS pname)");

    // Assert
    assert_binds(&cassie, &root, &sql);
}

#[test]
fn should_bind_an_outgoing_edge() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows]-> (b IS person) COLUMNS (e.since AS since)",
    );

    // Assert
    assert_binds(&cassie, &root, &sql);
}

#[test]
fn should_bind_an_incoming_edge() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let sql = graph_from(
        "social MATCH (a IS person) <-[e IS knows]- (b IS person) COLUMNS (b.id AS pid)",
    );

    // Assert
    assert_binds(&cassie, &root, &sql);
}

#[test]
fn should_bind_an_undirected_edge() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let sql =
        graph_from("social MATCH (a IS person) -[e IS knows]- (b IS person) COLUMNS (b.id AS pid)");

    // Assert
    assert_binds(&cassie, &root, &sql);
}

#[test]
fn should_bind_a_quantified_edge_with_an_inline_predicate() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows WHERE e.active = TRUE]->{1,3} (b IS person) \
         COLUMNS (a.id AS pid, b.id AS fid)",
    );

    // Assert
    assert_binds(&cassie, &root, &sql);
}

#[test]
fn should_bind_an_unbounded_quantifier_under_a_finite_mode() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows]->* (b IS person) KEEP ALL TRAIL PATHS \
         COLUMNS (b.id AS fid)",
    );

    // Assert
    assert_binds(&cassie, &root, &sql);
}

#[test]
fn should_bind_a_shortest_selector_with_a_pattern_filter() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let sql = graph_from(
        "social MATCH (a IS person) -[IS knows]->+ (b IS person) KEEP SHORTEST 2 ACYCLIC PATHS \
         WHERE a.id <> b.id COLUMNS (b.id AS fid)",
    );

    // Assert
    assert_binds(&cassie, &root, &sql);
}

#[test]
fn should_bind_a_repeated_singleton_variable() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let sql = graph_from(
        "social MATCH (a IS person) -[IS knows]-> (b IS person) -[IS knows]-> (a IS person) \
         COLUMNS (a.id AS pid)",
    );

    // Assert
    assert_binds(&cassie, &root, &sql);
}

#[test]
fn should_bind_properties_shared_by_a_label_alternative() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let sql = graph_from("social MATCH (x IS person|company) COLUMNS (x.id AS xid, x.name AS n)");

    // Assert
    assert_binds(&cassie, &root, &sql);
}

#[test]
fn should_bind_the_properties_of_a_shared_label() {
    // Arrange
    let (cassie, root) = fixture();

    // Act
    let sql = graph_from("social MATCH (x IS entity) COLUMNS (x.id AS xid, x.name AS n)");

    // Assert
    assert_binds(&cassie, &root, &sql);
}

#[test]
fn should_bind_parameters_in_predicates_or_columns() {
    // Arrange
    let (cassie, root) = fixture();
    let sql = graph_from(
        "social MATCH (a IS person WHERE a.id = $1) -[e IS knows WHERE e.since > $2]-> (b) \
         WHERE b.id <> $3 COLUMNS (a.id AS pid, $4 AS tag)",
    );
    let params = vec![
        Value::Int64(1),
        Value::String("2020".to_owned()),
        Value::Int64(2),
        Value::String("t".to_owned()),
    ];

    // Act
    let result = cassie.execute_sql(&root, &sql, params);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn should_select_a_mapping_that_carries_several_listed_labels_once() {
    // Arrange
    let (cassie, _root) = fixture();
    let graph = cassie
        .catalog
        .list_property_graphs()
        .into_iter()
        .next()
        .expect("graph");
    let requested = vec!["person".to_owned(), "entity".to_owned()];

    // Act
    let selected =
        select_elements(&graph, GraphVariableKind::Vertex, &requested).expect("labels exist");

    // Assert
    let people = selected
        .iter()
        .filter(|element| element.table.ends_with("people"))
        .count();
    assert_eq!(people, 1);
    assert_eq!(selected.len(), 2);
}

// Rejections.

/// The SQLSTATE and message `sql` fails with for the administrator.
fn rejected(sql: &str) -> (&'static str, String) {
    let (cassie, root) = fixture();
    failure(&cassie, &root, sql)
}

#[test]
fn should_reject_an_unknown_graph() {
    // Arrange
    let sql = graph_from("nowhere MATCH (a) COLUMNS (a.id AS pid)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42704", "{message}");
}

#[test]
fn should_reject_an_unknown_vertex_label() {
    // Arrange
    let sql = graph_from("social MATCH (a IS robot) COLUMNS (a.id AS pid)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42704", "{message}");
}

#[test]
fn should_reject_an_edge_label_on_a_vertex() {
    // Arrange
    let sql = graph_from("social MATCH (a IS knows) COLUMNS (a.id AS pid)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42704", "{message}");
}

#[test]
fn should_reject_an_unknown_property() {
    // Arrange
    let sql = graph_from("social MATCH (a IS person) COLUMNS (a.salary AS pay)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42703", "{message}");
}

#[test]
fn should_reject_a_property_missing_on_one_mapping() {
    // Arrange
    let sql = graph_from("social MATCH (x IS entity) COLUMNS (x.age AS years)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42703", "{message}");
}

#[test]
fn should_reject_a_property_with_incompatible_types() {
    // Arrange
    let sql = graph_from("social MATCH (x IS person|company) COLUMNS (x.age AS years)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42804", "{message}");
}

#[test]
fn should_reject_an_unknown_variable() {
    // Arrange
    let sql = graph_from("social MATCH (a IS person) COLUMNS (z.id AS pid)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42703", "{message}");
}

#[test]
fn should_reject_a_group_variable_in_columns() {
    // Arrange
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows]->{1,2} (b IS person) COLUMNS (e.since AS s)",
    );

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "0A000", "{message}");
}

#[test]
fn should_reject_a_group_variable_in_the_pattern_filter() {
    // Arrange
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows]->{1,2} (b IS person) WHERE e.active \
         COLUMNS (a.id AS pid)",
    );

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "0A000", "{message}");
}

#[test]
fn should_reject_a_group_variable_in_a_neighbouring_inline_predicate() {
    // Arrange
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows]->{1,2} (b IS person WHERE e.active) \
         COLUMNS (a.id AS pid)",
    );

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "0A000", "{message}");
}

#[test]
fn should_reject_a_repeated_group_variable() {
    // Arrange
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows]->{1,2} (b IS person) -[e IS knows]->{1,2} \
         (c IS person) COLUMNS (a.id AS pid)",
    );

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "0A000", "{message}");
}

#[test]
fn should_reject_a_variable_shared_by_a_vertex_with_an_edge() {
    // Arrange
    let sql = graph_from(
        "social MATCH (a IS person) -[a IS knows]-> (b IS person) COLUMNS (b.id AS pid)",
    );

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42712", "{message}");
}

#[test]
fn should_reject_a_path_variable_that_repeats_an_element_variable() {
    // Arrange
    let sql = graph_from("social MATCH a = (a IS person) COLUMNS (1 AS one)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42712", "{message}");
}

#[test]
fn should_reject_an_unbounded_star_under_walk() {
    // Arrange
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows]->* (b IS person) COLUMNS (b.id AS fid)",
    );

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "0A000", "{message}");
}

#[test]
fn should_reject_an_unbounded_plus_under_walk_with_a_selector() {
    // Arrange
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows]->+ (b IS person) KEEP ANY SHORTEST PATH \
         COLUMNS (b.id AS fid)",
    );

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "0A000", "{message}");
}

#[test]
fn should_reject_a_descending_quantifier_range() {
    // Arrange
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows]->{3,1} (b IS person) COLUMNS (b.id AS fid)",
    );

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "22023", "{message}");
}

#[test]
fn should_reject_a_zero_shortest_count() {
    // Arrange
    let sql = graph_from(
        "social MATCH (a IS person) -[e IS knows]->{1,2} (b IS person) KEEP SHORTEST 0 PATHS \
         COLUMNS (b.id AS fid)",
    );

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "22023", "{message}");
}

#[test]
fn should_reject_duplicate_output_columns() {
    // Arrange
    let sql = graph_from("social MATCH (a IS person) COLUMNS (a.id AS x, a.name AS x)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42701", "{message}");
}

#[test]
fn should_reject_a_subquery_inside_a_predicate() {
    // Arrange
    let sql =
        graph_from("social MATCH (a IS person WHERE EXISTS (SELECT 1)) COLUMNS (a.id AS pid)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "0A000", "{message}");
}

#[test]
fn should_reject_a_non_boolean_predicate() {
    // Arrange
    let sql = graph_from("social MATCH (a IS person WHERE a.name) COLUMNS (a.id AS pid)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "42804", "{message}");
}

#[test]
fn should_reject_an_element_variable_projected_whole() {
    // Arrange
    let sql = graph_from("social MATCH (a IS person) COLUMNS (a AS whole)");

    // Act
    let (state, message) = rejected(&sql);

    // Assert
    assert_eq!(state, "0A000", "{message}");
}

// Path-valued projections.

#[test]
fn should_report_path_valued_columns_as_not_yet_supported() {
    // Arrange
    let (cassie, root) = fixture();
    let sql = graph_from(
        "social MATCH p = (a IS person) -[e IS knows]->{0,2} (b IS person) COLUMNS (p AS path)",
    );

    // Act
    let (state, message) = failure(&cassie, &root, &sql);

    // Assert
    assert_eq!(state, "0A000");
    assert!(message.contains("graph_path"));
}

// Output schema.

#[test]
fn should_describe_the_declared_column_types() {
    // Arrange
    let (cassie, _root) = fixture();
    let sql =
        "SELECT * FROM GRAPH_TABLE (social MATCH (a IS person) -[e IS knows]-> (b IS person) \
               COLUMNS (a.id AS pid, a.name AS pname, a.age AS years, e.active AS live)) AS g";

    // Act
    let columns = cassie.describe_sql(sql).expect("describes");

    // Assert
    let described = columns
        .iter()
        .map(|column| (column.name.as_str(), column.data_type.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        described,
        vec![
            ("pid", "bigint"),
            ("pname", "text"),
            ("years", "int"),
            ("live", "boolean"),
        ]
    );
}

#[test]
fn should_name_a_bare_property_column_after_its_property() {
    // Arrange
    let (cassie, _root) = fixture();
    let sql = "SELECT * FROM GRAPH_TABLE (social MATCH (a IS person) COLUMNS (a.name)) AS g";

    // Act
    let columns = cassie.describe_sql(sql).expect("describes");

    // Assert
    assert_eq!(columns[0].name, "name");
}

#[test]
fn should_apply_a_column_alias_list_to_the_output() {
    // Arrange
    let (cassie, _root) = fixture();
    let sql = "SELECT * FROM GRAPH_TABLE (social MATCH (a IS person) COLUMNS (a.id AS pid, \
               a.name AS pname)) AS g (x, y)";

    // Act
    let columns = cassie.describe_sql(sql).expect("describes");

    // Assert
    let names = columns
        .iter()
        .map(|column| column.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["x", "y"]);
}

// Composition with the rest of SELECT.

#[test]
fn should_compose_with_a_join() {
    // Arrange
    let (cassie, root) = fixture();
    let sql = "SELECT p.name FROM people AS p JOIN GRAPH_TABLE (social MATCH (a IS person) \
               COLUMNS (a.id AS pid)) AS g ON g.pid = p.id";

    // Act
    let described = cassie.describe_sql(sql);

    // Assert
    assert!(
        described.is_ok(),
        "{}",
        described
            .as_ref()
            .err()
            .map_or_else(String::new, ToString::to_string)
    );
    assert_binds(&cassie, &root, sql);
}

#[test]
fn should_compose_with_a_lateral_reference_to_a_preceding_table() {
    // Arrange
    let (cassie, root) = fixture();
    let sql = "SELECT p.name, g.friend FROM people AS p, LATERAL GRAPH_TABLE (social MATCH \
               (a IS person WHERE a.id = p.id) -[IS knows]-> (b IS person) \
               COLUMNS (b.name AS friend)) AS g";

    // Act
    let described = cassie.describe_sql(sql);

    // Assert
    assert!(
        described.is_ok(),
        "{}",
        described
            .as_ref()
            .err()
            .map_or_else(String::new, ToString::to_string)
    );
    assert_binds(&cassie, &root, sql);
}

#[test]
fn should_reject_an_outer_reference_without_lateral() {
    // Arrange
    let sql = "SELECT p.name FROM people AS p, GRAPH_TABLE (social MATCH \
               (a IS person WHERE a.id = p.id) COLUMNS (a.name AS friend)) AS g";

    // Act
    let (state, message) = rejected(sql);

    // Assert
    assert_eq!(state, "42703", "{message}");
}

#[test]
fn should_compose_with_aggregation() {
    // Arrange
    let (cassie, root) = fixture();
    let sql = "SELECT g.pname, count(*) FROM GRAPH_TABLE (social MATCH (a IS person) \
               COLUMNS (a.name AS pname)) AS g GROUP BY g.pname";

    // Act
    let described = cassie.describe_sql(sql);

    // Assert
    assert!(
        described.is_ok(),
        "{}",
        described
            .as_ref()
            .err()
            .map_or_else(String::new, ToString::to_string)
    );
    assert_binds(&cassie, &root, sql);
}

#[test]
fn should_compose_with_a_derived_table() {
    // Arrange
    let (cassie, root) = fixture();
    let sql = "SELECT s.pid FROM (SELECT g.pid FROM GRAPH_TABLE (social MATCH (a IS person) \
               COLUMNS (a.id AS pid)) AS g) AS s";

    // Act
    let described = cassie.describe_sql(sql);

    // Assert
    assert!(
        described.is_ok(),
        "{}",
        described
            .as_ref()
            .err()
            .map_or_else(String::new, ToString::to_string)
    );
    assert_binds(&cassie, &root, sql);
}

#[test]
fn should_compose_with_exists() {
    // Arrange
    let (cassie, root) = fixture();
    let sql = "SELECT p.id FROM people AS p WHERE EXISTS (SELECT 1 FROM GRAPH_TABLE (social \
               MATCH (a IS person) COLUMNS (a.id AS pid)) AS g WHERE g.pid = p.id)";

    // Act
    let described = cassie.describe_sql(sql);

    // Assert
    assert!(
        described.is_ok(),
        "{}",
        described
            .as_ref()
            .err()
            .map_or_else(String::new, ToString::to_string)
    );
    assert_binds(&cassie, &root, sql);
}

// Authorization.

fn reader(cassie: &Cassie, root: &CassieSession, tables: &[&str]) -> CassieSession {
    let password = uuid::Uuid::new_v4().to_string();
    let mut statements = vec![
        format!("CREATE ROLE reader LOGIN PASSWORD '{password}'"),
        "GRANT USAGE ON SCHEMA public TO reader".to_owned(),
    ];
    statements.extend(
        tables
            .iter()
            .map(|table| format!("GRANT SELECT ON TABLE public.{table} TO reader")),
    );
    for sql in statements {
        cassie.execute_sql(root, &sql, vec![]).expect("grant");
    }
    cassie
        .authenticate_role("reader", Some(&password), None)
        .expect("reader login")
}

#[test]
fn should_require_select_on_a_vertex_table() {
    // Arrange
    let (cassie, root) = fixture();
    let reader = reader(&cassie, &root, &["follows"]);
    let sql = graph_from("social MATCH (a IS person) COLUMNS (a.id AS pid)");

    // Act
    let denied = failure(&cassie, &reader, &sql);
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.people TO reader",
            vec![],
        )
        .expect("grant");
    let granted = cassie.execute_sql(&reader, &sql, vec![]);

    // Assert
    assert_eq!(denied.0, "42501");
    assert!(granted.is_ok());
}

#[test]
fn should_require_select_on_an_edge_table() {
    // Arrange
    let (cassie, root) = fixture();
    let reader = reader(&cassie, &root, &["people"]);
    let sql =
        graph_from("social MATCH (a IS person) -[IS knows]-> (b IS person) COLUMNS (b.id AS pid)");

    // Act
    let denied = failure(&cassie, &reader, &sql);
    cassie
        .execute_sql(
            &root,
            "GRANT SELECT ON TABLE public.follows TO reader",
            vec![],
        )
        .expect("grant");
    let granted = cassie.execute_sql(&reader, &sql, vec![]);

    // Assert
    assert_eq!(denied.0, "42501");
    assert!(granted.is_ok());
}

#[test]
fn should_require_select_on_every_table_an_unlabelled_vertex_can_read() {
    // Arrange
    let (cassie, root) = fixture();
    let reader = reader(&cassie, &root, &["people", "follows"]);
    let sql = graph_from("social MATCH (a) COLUMNS (a.id AS pid)");

    // Act
    let denied = failure(&cassie, &reader, &sql);

    // Assert
    assert_eq!(denied.0, "42501");
}

#[test]
fn should_not_require_select_on_a_table_a_label_does_not_select() {
    // Arrange
    let (cassie, root) = fixture();
    let reader = reader(&cassie, &root, &["people"]);
    let sql = graph_from("social MATCH (a IS person) COLUMNS (a.id AS pid)");

    // Act
    let result = cassie.execute_sql(&reader, &sql, vec![]);

    // Assert
    assert!(result.is_ok());
}

// Views.

#[test]
fn should_record_the_graph_as_a_view_dependency() {
    // Arrange
    let parsed = crate::sql::parser::parse_statement(
        "SELECT g.pid FROM GRAPH_TABLE (social MATCH (a IS person) COLUMNS (a.id AS pid)) AS g",
    )
    .expect("parses");
    let crate::sql::QueryStatement::Select(select) = parsed.statement else {
        panic!("expected a select");
    };

    // Act
    let relations = super::select_referenced_graphs(&select);

    // Assert
    assert_eq!(relations, vec!["social".to_owned()]);
}

#[test]
fn should_query_a_view_over_a_graph_table() {
    // Arrange
    let (cassie, root) = fixture();
    cassie
        .execute_sql(
            &root,
            "CREATE VIEW friends AS SELECT g.pid FROM GRAPH_TABLE (social MATCH (a IS person) \
             COLUMNS (a.id AS pid)) AS g",
            vec![],
        )
        .expect("view");

    // Act
    let rows = succeed(&cassie, &root, "SELECT * FROM friends");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

const GRAPH_VIEW: &str = "CREATE VIEW friends AS SELECT g.pid FROM GRAPH_TABLE (social MATCH \
                          (a IS person) COLUMNS (a.id AS pid)) AS g";

#[test]
fn should_refuse_dropping_a_graph_that_a_view_reads() {
    // Arrange
    let (cassie, root) = fixture();
    cassie.execute_sql(&root, GRAPH_VIEW, vec![]).expect("view");

    // Act
    let (state, message) = failure(&cassie, &root, "DROP PROPERTY GRAPH social");

    // Assert
    assert_eq!(state, "2BP01", "{message}");
}

#[test]
fn should_drop_a_graph_after_its_view_is_dropped() {
    // Arrange
    let (cassie, root) = fixture();
    cassie.execute_sql(&root, GRAPH_VIEW, vec![]).expect("view");
    cassie
        .execute_sql(&root, "DROP VIEW friends", vec![])
        .expect("drop view");

    // Act
    let dropped = cassie.execute_sql(&root, "DROP PROPERTY GRAPH social", vec![]);

    // Assert
    assert!(dropped.is_ok());
}

#[test]
fn should_refuse_dropping_a_mapped_table_under_a_view_over_its_graph() {
    // Arrange
    let (cassie, root) = fixture();
    cassie.execute_sql(&root, GRAPH_VIEW, vec![]).expect("view");

    // Act
    let (state, message) = failure(&cassie, &root, "DROP TABLE people");

    // Assert
    assert_eq!(state, "2BP01", "{message}");
}
