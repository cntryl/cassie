//! Path constructors, `key_parts`, `path_steps`, `weighted_path_steps`,
//! `path_to_jsonb` and structural validation.

use crate::fixture::{one_hop, person, started, SOCIAL_SETUP};
use cassie::types::Value;

fn social(label: &str) -> crate::fixture::GraphFixture {
    started(label, &SOCIAL_SETUP)
}

#[test]
fn should_reject_a_path_with_the_wrong_vertex_count_with_22023() {
    // Arrange
    let fixture = social("graph_paths_shape");

    // Act
    let state = fixture.sqlstate(&format!(
        "SELECT ROW(ARRAY[{}], ARRAY[]::cassie_graph.graph_element_ref[], ARRAY[1::smallint])::cassie_graph.graph_path",
        person(2)
    ));

    // Assert
    assert_eq!(state, "22023");
}

#[test]
fn should_accept_a_zero_hop_path() {
    // Arrange
    let fixture = social("graph_paths_zero");

    // Act
    let rows = fixture.rows(&format!(
        "SELECT cardinality((ROW(ARRAY[{}], ARRAY[]::cassie_graph.graph_element_ref[], \
         ARRAY[]::smallint[])::cassie_graph.graph_path).vertices)",
        person(2)
    ));

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_list_the_key_parts_of_a_reference_from_ordinal_one() {
    // Arrange
    let fixture = social("graph_paths_key_parts");

    // Act
    let rows = fixture.rows(&format!(
        "SELECT ordinal, type_tag FROM cassie_graph.key_parts({})",
        person(9_007_199_254_740_993)
    ));

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1), Value::Int64(3)]]);
}

#[test]
fn should_start_path_steps_at_position_zero_with_no_edge_or_direction() {
    // Arrange
    let fixture = social("graph_paths_first_step");

    // Act
    let rows = fixture.rows(&format!(
        "SELECT position, direction, edge IS NULL FROM cassie_graph.path_steps({}) ORDER BY position",
        one_hop(2, 2, 1)
    ));

    // Assert
    assert_eq!(rows[0], vec![Value::Int64(0), Value::Null, Value::Bool(true)]);
    assert_eq!(rows[1][1], Value::Int64(1));
}

#[test]
fn should_return_one_row_for_a_zero_hop_path_in_path_steps() {
    // Arrange
    let fixture = social("graph_paths_zero_steps");

    // Act
    let rows = fixture.rows(&format!(
        "SELECT count(*) FROM cassie_graph.path_steps(ROW(ARRAY[{}], \
         ARRAY[]::cassie_graph.graph_element_ref[], ARRAY[]::smallint[])::cassie_graph.graph_path)",
        person(2)
    ));

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(1)]]);
}

#[test]
fn should_return_no_rows_for_a_null_path() {
    // Arrange
    let fixture = social("graph_paths_null");

    // Act
    let rows = fixture.rows("SELECT * FROM cassie_graph.path_steps(NULL)");

    // Assert
    assert!(rows.is_empty());
}

#[test]
fn should_accumulate_costs_along_a_weighted_path() {
    // Arrange
    let fixture = social("graph_paths_weighted");

    // Act
    let rows = fixture.rows(&format!(
        "SELECT cumulative_cost::text FROM cassie_graph.weighted_path_steps({}, ARRAY[1.5::numeric]) \
         ORDER BY position",
        one_hop(2, 2, 1)
    ));

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::String("0".into())],
            vec![Value::String("1.5".into())]
        ]
    );
}

#[test]
fn should_reject_a_negative_edge_cost_with_22023() {
    // Arrange
    let fixture = social("graph_paths_negative");

    // Act
    let state = fixture.sqlstate(&format!(
        "SELECT * FROM cassie_graph.weighted_path_steps({}, ARRAY[-1::numeric])",
        one_hop(2, 2, 1)
    ));

    // Assert
    assert_eq!(state, "22023");
}

#[test]
fn should_reject_a_cost_array_of_the_wrong_length_with_22023() {
    // Arrange
    let fixture = social("graph_paths_length");

    // Act
    let state = fixture.sqlstate(&format!(
        "SELECT * FROM cassie_graph.weighted_path_steps({}, ARRAY[1::numeric, 2::numeric])",
        one_hop(2, 2, 1)
    ));

    // Assert
    assert_eq!(state, "22023");
}

#[test]
fn should_export_a_zero_hop_path_as_version_one_json() {
    // Arrange
    let fixture = social("graph_paths_json");

    // Act
    let rows = fixture.rows(&format!(
        "SELECT cassie_graph.path_to_jsonb(ROW(ARRAY[{}], \
         ARRAY[]::cassie_graph.graph_element_ref[], ARRAY[]::smallint[])::cassie_graph.graph_path)",
        person(9_007_199_254_740_993)
    ));

    // Assert
    let Value::Json(document) = &rows[0][0] else {
        panic!("expected a JSON document");
    };
    assert_eq!(document["version"], 1);
    assert_eq!(
        document["vertices"][0]["key_parts"][0],
        serde_json::json!({"type": "bigint", "value": "9007199254740993"})
    );
}

#[test]
fn should_return_null_for_a_null_path_in_path_to_jsonb() {
    // Arrange
    let fixture = social("graph_paths_json_null");

    // Act
    let rows = fixture.rows("SELECT cassie_graph.path_to_jsonb(NULL)");

    // Assert
    assert_eq!(rows, vec![vec![Value::Null]]);
}

#[test]
fn should_order_forward_before_reverse_paths_with_order_by() {
    // Arrange
    let fixture = social("graph_paths_order");
    fixture.run("CREATE TABLE routes (id INT PRIMARY KEY, p cassie_graph.graph_path)");
    fixture.run(&format!("INSERT INTO routes VALUES (1, {})", one_hop(2, 2, 2)));
    fixture.run(&format!("INSERT INTO routes VALUES (2, {})", one_hop(2, 2, 1)));

    // Act
    let rows = fixture.rows("SELECT id FROM routes ORDER BY p");

    // Assert
    assert_eq!(rows, vec![vec![Value::Int64(2)], vec![Value::Int64(1)]]);
}
