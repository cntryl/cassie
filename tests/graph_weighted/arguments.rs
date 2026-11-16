//! Argument validation of `graph_weighted_paths`, with the contract's SQLSTATEs.

use crate::world::{fast_edge, vertex, Wg};

fn world() -> Wg {
    Wg::new(3, &[crate::world::fast(1, 1, 2, 1), crate::world::slow(2, 2, 3, 2)])
}

/// SQLSTATE of the call `'wg', source 1, target 2, <rest>`.
fn state(world: &Wg, rest: &str) -> String {
    match world.call(1, 2, rest) {
        Ok(_) => "success".to_owned(),
        Err(state) => state,
    }
}

fn state_of_arguments(world: &Wg, arguments: &str) -> String {
    match world.call_with(arguments) {
        Ok(_) => "success".to_owned(),
        Err(state) => state,
    }
}

#[test]
fn should_reject_a_null_required_argument_with_22004() {
    // Arrange
    let world = world();
    let (one, two) = (vertex(1), vertex(2));
    let calls = [
        format!("NULL, {one}, {two}, 'weight', 1"),
        format!("'wg', NULL, {two}, 'weight', 1"),
        format!("'wg', {one}, NULL, 'weight', 1"),
        format!("'wg', {one}, {two}, NULL, 1"),
        format!("'wg', {one}, {two}, 'weight', NULL"),
    ];

    // Act
    let states = calls.iter().map(|call| state_of_arguments(&world, call)).collect::<Vec<_>>();

    // Assert
    assert_eq!(states, vec!["22004"; 5]);
}

#[test]
fn should_reject_an_explicit_null_direction_or_k_with_22004() {
    // Arrange
    let world = world();

    // Act
    let direction = state(&world, "'weight', 1, NULL");
    let k = state(&world, "'weight', 1, 'out', NULL, NULL, NULL");

    // Assert
    assert_eq!((direction.as_str(), k.as_str()), ("22004", "22004"));
}

#[test]
fn should_reject_a_null_element_in_the_label_array_with_22004() {
    // Arrange
    let world = world();

    // Act
    let result = state(&world, "'weight', 1, 'out', ARRAY['fast', NULL]");

    // Assert
    assert_eq!(result, "22004");
}

#[test]
fn should_reject_invalid_bounds_and_direction_with_22023() {
    // Arrange
    let world = world();
    let calls = [
        "'weight', -1",
        "'weight', 1, 'sideways'",
        "'weight', 1, 'OUT'",
        "'weight', 1, 'out', NULL, NULL, 0",
        "'weight', 1, 'out', NULL, NULL, -3",
        "'weight', 1, 'out', NULL, -0.5",
        "'weight', 1, 'out', NULL, 'NaN'::numeric",
    ];

    // Act
    let states = calls.iter().map(|rest| state(&world, rest)).collect::<Vec<_>>();

    // Assert
    assert_eq!(states, vec!["22023"; calls.len()]);
}

#[test]
fn should_reject_an_edge_reference_as_source_or_target_with_22023() {
    // Arrange
    let world = world();
    let edge = fast_edge(1);
    let calls = [
        format!("'wg', {edge}, {}, 'weight', 1", vertex(2)),
        format!("'wg', {}, {edge}, 'weight', 1", vertex(1)),
    ];

    // Act
    let states = calls.iter().map(|call| state_of_arguments(&world, call)).collect::<Vec<_>>();

    // Assert
    assert_eq!(states, vec!["22023"; 2]);
}

#[test]
fn should_reject_an_unknown_graph_or_label_with_42704() {
    // Arrange
    let world = world();
    let (one, two) = (vertex(1), vertex(2));

    // Act
    let graph = state_of_arguments(&world, &format!("'missing', {one}, {two}, 'weight', 1"));
    let label = state(&world, "'weight', 1, 'out', ARRAY['nope']");

    // Assert
    assert_eq!((graph.as_str(), label.as_str()), ("42704", "42704"));
}

#[test]
fn should_reject_a_reference_to_another_graph_with_42704() {
    // Arrange
    let world = world();
    let other = "cassie_graph.vertex_ref('other', 'n', ARRAY[cassie_graph.key_part(1::bigint)])";

    // Act
    let result = state_of_arguments(&world, &format!("'wg', {other}, {}, 'weight', 1", vertex(2)));

    // Assert
    assert_eq!(result, "42704");
}

#[test]
fn should_reject_a_reference_to_a_missing_vertex_with_42704() {
    // Arrange
    let world = world();

    let one = vertex(1);
    let ghost = format!(
        "ROW(({one}).database_id, ({one}).graph_id, ({one}).mapping_id, 1::smallint, \
         ARRAY[cassie_graph.key_part(99::bigint)])::cassie_graph.graph_element_ref"
    );

    // Act
    let result = state_of_arguments(&world, &format!("'wg', {one}, {ghost}, 'weight', 1"));

    // Assert
    assert_eq!(result, "42704");
}

#[test]
fn should_reject_an_unknown_weight_property_with_42703() {
    // Arrange
    let world = world();

    // Act
    let unknown = state(&world, "'nothing', 1");
    let missing_on_one_mapping = state(&world, "'extra', 1");

    // Assert
    assert_eq!((unknown.as_str(), missing_on_one_mapping.as_str()), ("42703", "42703"));
}

#[test]
fn should_accept_a_property_that_only_the_selected_mappings_carry() {
    // Arrange
    let world = world();

    // Act
    let restricted = world.call(2, 3, "'extra', 1, 'out', ARRAY['slow']");

    // Assert
    assert_eq!(restricted.expect("call succeeds").len(), 1);
}

#[test]
fn should_reject_incompatible_declared_weight_types_with_42804() {
    // Arrange
    let world = world();

    // Act
    let text = state(&world, "'note', 1, 'out', ARRAY['fast']");
    let float = state(&world, "'ratio', 1, 'out', ARRAY['fast']");

    // Assert
    assert_eq!((text.as_str(), float.as_str()), ("42804", "42804"));
}

#[test]
fn should_reject_wrongly_typed_arguments_with_42804() {
    // Arrange
    let world = world();
    let (one, two) = (vertex(1), vertex(2));
    let calls = [
        format!("'wg', 1, {two}, 'weight', 1"),
        format!("'wg', {one}, {two}, 'weight', 'many'"),
        format!("'wg', {one}, {two}, 'weight', 1, 'out', 'fast'"),
    ];

    // Act
    let states = calls.iter().map(|call| state_of_arguments(&world, call)).collect::<Vec<_>>();

    // Assert
    assert_eq!(states, vec!["42804"; 3]);
}
