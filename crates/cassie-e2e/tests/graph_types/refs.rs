//! `vertex_ref` and `edge_ref`: resolution, rejection classes, transaction
//! visibility, authority and identity across graph rename and recreation.

use crate::fixture::{knows, person, started, SOCIAL_SETUP};
use cassie::types::graph::{ElementKind, ElementRef, KeyPart};
use cassie::types::Value;

fn social(label: &str) -> crate::fixture::GraphFixture {
    started(label, &SOCIAL_SETUP)
}

/// The reference `expression` yields, rebuilt from its fields: a result row
/// carries the record's text form.
pub(super) fn reference_of(
    fixture: &crate::fixture::GraphFixture,
    expression: &str,
    key: i64,
) -> ElementRef {
    let rows = fixture.rows(&format!(
        "SELECT (r).database_id, (r).graph_id, (r).mapping_id, (r).element_kind \
         FROM (SELECT {expression} AS r) AS t"
    ));
    let text = |value: &Value| match value {
        Value::String(text) => uuid::Uuid::parse_str(text).expect("uuid"),
        _ => panic!("expected uuid text"),
    };
    ElementRef {
        database_id: text(&rows[0][0]),
        graph_id: text(&rows[0][1]),
        mapping_id: text(&rows[0][2]),
        kind: ElementKind::Vertex,
        key: vec![KeyPart::BigInt(key)],
    }
}

#[test]
fn should_resolve_an_existing_vertex() {
    // Arrange
    let fixture = social("graph_refs_vertex");

    // Act
    let rows = fixture.rows(&format!("SELECT ({}).element_kind", person(2)));

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_resolve_an_existing_edge() {
    // Arrange
    let fixture = social("graph_refs_edge");

    // Act
    let rows = fixture.rows(&format!("SELECT ({}).element_kind", knows(10)));

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_resolve_a_schema_qualified_graph_name() {
    // Arrange
    let fixture = social("graph_refs_qualified");

    // Act
    let rows = fixture.rows(
        "SELECT (cassie_graph.vertex_ref('public.social', 'person', \
         ARRAY[cassie_graph.key_part(2::bigint)])).element_kind",
    );

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_reject_an_unknown_graph_with_42704() {
    // Arrange
    let fixture = social("graph_refs_unknown_graph");

    // Act
    let state = fixture.sqlstate(
        "SELECT cassie_graph.vertex_ref('absent', 'person', ARRAY[cassie_graph.key_part(2::bigint)])",
    );

    // Assert
    assert_eq!(state, "42704");
}

#[test]
fn should_reject_an_unknown_mapping_with_42704() {
    // Arrange
    let fixture = social("graph_refs_unknown_mapping");

    // Act
    let state = fixture.sqlstate(
        "SELECT cassie_graph.vertex_ref('social', 'absent', ARRAY[cassie_graph.key_part(2::bigint)])",
    );

    // Assert
    assert_eq!(state, "42704");
}

#[test]
fn should_reject_an_edge_mapping_used_as_a_vertex_mapping_with_42704() {
    // Arrange
    let fixture = social("graph_refs_wrong_kind");

    // Act
    let state = fixture.sqlstate(
        "SELECT cassie_graph.vertex_ref('social', 'knows', ARRAY[cassie_graph.key_part(10::bigint)])",
    );

    // Assert
    assert_eq!(state, "42704");
}

#[test]
fn should_reject_an_unknown_element_with_42704() {
    // Arrange
    let fixture = social("graph_refs_unknown_element");

    // Act
    let state = fixture.sqlstate(&format!("SELECT {}", person(404)));

    // Assert
    assert_eq!(state, "42704");
}

#[test]
fn should_reject_a_key_type_mismatch_with_42804() {
    // Arrange
    let fixture = social("graph_refs_type_mismatch");

    // Act
    let state = fixture.sqlstate(
        "SELECT cassie_graph.vertex_ref('social', 'person', ARRAY[cassie_graph.key_part(2::integer)])",
    );

    // Assert
    assert_eq!(state, "42804");
}

#[test]
fn should_reject_a_key_arity_mismatch_with_42804() {
    // Arrange
    let fixture = social("graph_refs_arity");

    // Act
    let state = fixture.sqlstate(
        "SELECT cassie_graph.vertex_ref('social', 'person', \
         ARRAY[cassie_graph.key_part(2::bigint), cassie_graph.key_part(3::bigint)])",
    );

    // Assert
    assert_eq!(state, "42804");
}

#[test]
fn should_reject_a_null_graph_name_with_22004() {
    // Arrange
    let fixture = social("graph_refs_null_graph");

    // Act
    let state = fixture.sqlstate(
        "SELECT cassie_graph.vertex_ref(NULL, 'person', ARRAY[cassie_graph.key_part(2::bigint)])",
    );

    // Assert
    assert_eq!(state, "22004");
}

#[test]
fn should_reject_a_null_key_array_with_22004() {
    // Arrange
    let fixture = social("graph_refs_null_key");

    // Act
    let state = fixture.sqlstate("SELECT cassie_graph.vertex_ref('social', 'person', NULL)");

    // Assert
    assert_eq!(state, "22004");
}

#[test]
fn should_see_a_row_inserted_in_the_current_transaction() {
    // Arrange
    let fixture = social("graph_refs_tx_insert");
    fixture.run("BEGIN");
    fixture.run("INSERT INTO people VALUES (3, 'c')");

    // Act
    let rows = fixture.rows(&format!("SELECT ({}).element_kind", person(3)));

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_not_see_a_row_deleted_in_the_current_transaction() {
    // Arrange
    let fixture = social("graph_refs_tx_delete");
    fixture.run("BEGIN");
    fixture.run("DELETE FROM people WHERE id = 2");

    // Act
    let state = fixture.sqlstate(&format!("SELECT {}", person(2)));

    // Assert
    assert_eq!(state, "42704");
}

#[test]
fn should_forget_a_row_whose_insert_was_rolled_back() {
    // Arrange
    let fixture = social("graph_refs_tx_rollback");
    fixture.run("BEGIN");
    fixture.run("INSERT INTO people VALUES (3, 'c')");
    fixture.run("ROLLBACK");

    // Act
    let state = fixture.sqlstate(&format!("SELECT {}", person(3)));

    // Assert
    assert_eq!(state, "42704");
}

#[test]
fn should_keep_a_reference_resolvable_after_the_graph_is_renamed() {
    // Arrange
    let fixture = social("graph_refs_rename");
    let before = reference_of(&fixture, &person(2), 2);
    fixture.run("ALTER PROPERTY GRAPH social RENAME TO friends");

    // Act
    let resolved = fixture
        .cassie
        .catalog
        .resolve_element_ref(&before, "postgres");
    let after = reference_of(
        &fixture,
        "cassie_graph.vertex_ref('friends', 'person', ARRAY[cassie_graph.key_part(2::bigint)])",
        2,
    );

    // Assert
    assert!(resolved.is_ok());
    assert_eq!(before, after);
}

#[test]
fn should_resolve_an_old_reference_to_the_replacement_after_reinserting_its_key() {
    // Arrange
    let fixture = social("graph_refs_reinsert");
    let before = reference_of(&fixture, &person(2), 2);

    // Act
    fixture.run("DELETE FROM people WHERE id = 2");
    fixture.run("INSERT INTO people VALUES (2, 'replacement')");
    let after = reference_of(&fixture, &person(2), 2);
    let resolved = fixture
        .cassie
        .catalog
        .resolve_element_ref(&before, "postgres");
    let rows = fixture.rows(
        "SELECT g.name FROM GRAPH_TABLE (social MATCH (p IS person WHERE p.id = 2) \
         COLUMNS (p.name AS name)) AS g",
    );

    // Assert
    assert_eq!(before, after);
    assert!(resolved.is_ok());
    assert_eq!(rows, vec![vec![Value::String("replacement".to_owned())]]);
}

#[test]
fn should_make_an_old_reference_unresolvable_after_the_graph_is_dropped_then_recreated() {
    // Arrange
    let fixture = social("graph_refs_recreate");
    let old = reference_of(&fixture, &person(2), 2);
    fixture.run("DROP PROPERTY GRAPH social");
    fixture.run(SOCIAL_SETUP[4]);

    // Act
    let outcome = fixture.cassie.catalog.resolve_element_ref(&old, "postgres");
    let fresh = reference_of(&fixture, &person(2), 2);

    // Assert
    assert_eq!(outcome.expect_err("stale reference").sql_state(), "42704");
    assert_ne!(old, fresh);
}

#[test]
fn should_deny_a_role_without_select_on_the_mapped_table_with_42501() {
    // Arrange
    let fixture = social("graph_refs_authority");
    let password = crate::support_sql::test_root_password();
    fixture.run(&format!("CREATE ROLE reader LOGIN PASSWORD '{password}'"));
    let reader = fixture
        .cassie
        .authenticate_role("reader", Some(password), Some("postgres".to_owned()))
        .expect("role login");

    // Act
    let error = fixture
        .cassie
        .execute_sql(&reader, &format!("SELECT {}", person(2)), Vec::new())
        .expect_err("no access");

    // Assert
    assert_eq!(error.sql_state(), "42501");
}

#[test]
fn should_not_reveal_whether_a_protected_row_exists_to_a_role_without_select() {
    // Arrange
    let fixture = social("graph_refs_no_leak");
    let password = crate::support_sql::test_root_password();
    fixture.run(&format!("CREATE ROLE reader LOGIN PASSWORD '{password}'"));
    let reader = fixture
        .cassie
        .authenticate_role("reader", Some(password), Some("postgres".to_owned()))
        .expect("role login");

    // Act
    let states = [2, 404].map(|id| {
        fixture
            .cassie
            .execute_sql(&reader, &format!("SELECT {}", person(id)), Vec::new())
            .expect_err("no access")
            .sql_state()
    });

    // Assert
    assert_eq!(states, ["42501", "42501"]);
}
