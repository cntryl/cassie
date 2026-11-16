//! Positional, named and mixed calls, defaults and argument-name errors.

use crate::world::{vertex, Wg};

fn world() -> Wg {
    Wg::new(
        3,
        &[
            crate::world::fast(1, 1, 2, 1),
            crate::world::fast(2, 2, 3, 1),
            crate::world::fast(3, 1, 3, 5),
        ],
    )
}

fn outcome(world: &Wg, arguments: &str) -> String {
    match world.call_with(arguments) {
        Ok(_) => "success".to_owned(),
        Err(state) => state,
    }
}

#[test]
fn should_apply_the_defaults_for_omitted_trailing_arguments() {
    // Arrange
    let world = world();

    // Act
    let defaulted = world.call(1, 3, "'weight', 2").expect("call succeeds");
    let explicit = world
        .call(1, 3, "'weight', 2, 'out', NULL, NULL, 1")
        .expect("call succeeds");

    // Assert
    assert_eq!(defaulted, explicit);
    assert_eq!(defaulted.len(), 1);
}

#[test]
fn should_bind_named_arguments_in_any_order() {
    // Arrange
    let world = world();
    let positional = world.call(1, 3, "'weight', 2, 'out', NULL, NULL, 2").expect("call succeeds");
    let named = format!(
        "k => 2, max_hops => 2, target => {}, source => {}, graph => 'wg', weight_property => 'weight'",
        vertex(3),
        vertex(1)
    );

    // Act
    let rows = world.call_with(&named).expect("call succeeds");

    // Assert
    assert_eq!(rows, positional);
    assert_eq!(rows.len(), 2);
}

#[test]
fn should_bind_positional_arguments_followed_by_named_ones() {
    // Arrange
    let world = world();
    let expected = world.call(1, 3, "'weight', 2, 'both', NULL, NULL, 2").expect("call succeeds");
    let mixed = format!(
        "'wg', {}, {}, 'weight', 2, k => 2, direction => 'both'",
        vertex(1),
        vertex(3)
    );

    // Act
    let rows = world.call_with(&mixed).expect("call succeeds");

    // Assert
    assert_eq!(rows, expected);
}

#[test]
fn should_distinguish_an_explicit_null_from_an_omitted_named_argument() {
    // Arrange
    let world = world();
    let base = format!(
        "graph => 'wg', source => {}, target => {}, weight_property => 'weight', max_hops => 2",
        vertex(1),
        vertex(3)
    );

    // Act
    let omitted = outcome(&world, &base);
    let null_k = outcome(&world, &format!("{base}, k => NULL"));
    let null_cost = outcome(&world, &format!("{base}, max_cost => NULL"));

    // Assert
    assert_eq!((omitted.as_str(), null_k.as_str(), null_cost.as_str()), ("success", "22004", "success"));
}

#[test]
fn should_match_argument_names_without_regard_to_case() {
    // Arrange
    let world = world();
    let call = format!(
        "GRAPH => 'wg', Source => {}, TARGET => {}, weight_property => 'weight', MAX_HOPS => 1",
        vertex(1),
        vertex(2)
    );

    // Act
    let result = outcome(&world, &call);

    // Assert
    assert_eq!(result, "success");
}

#[test]
fn should_reject_a_duplicate_argument_name_with_42601() {
    // Arrange
    let world = world();
    let call = format!(
        "graph => 'wg', source => {}, target => {}, weight_property => 'weight', max_hops => 1, k => 1, k => 2",
        vertex(1),
        vertex(2)
    );

    // Act
    let result = outcome(&world, &call);

    // Assert
    assert_eq!(result, "42601");
}

#[test]
fn should_reject_a_name_that_repeats_a_positional_argument_with_42601() {
    // Arrange
    let world = world();
    let call = format!(
        "'wg', {}, {}, 'weight', 1, graph => 'wg'",
        vertex(1),
        vertex(2)
    );

    // Act
    let result = outcome(&world, &call);

    // Assert
    assert_eq!(result, "42601");
}

#[test]
fn should_reject_a_positional_argument_after_a_named_one_with_42601() {
    // Arrange
    let world = world();
    let call = format!("graph => 'wg', {}, {}, 'weight', 1", vertex(1), vertex(2));

    // Act
    let result = outcome(&world, &call);

    // Assert
    assert_eq!(result, "42601");
}

#[test]
fn should_reject_an_unknown_argument_name_with_42883() {
    // Arrange
    let world = world();
    let call = format!(
        "graph => 'wg', source => {}, target => {}, weight_property => 'weight', max_hops => 1, limit => 3",
        vertex(1),
        vertex(2)
    );

    // Act
    let result = outcome(&world, &call);

    // Assert
    assert_eq!(result, "42883");
}

#[test]
fn should_reject_a_call_without_a_required_argument_with_42883() {
    // Arrange
    let world = world();
    let missing = format!("graph => 'wg', source => {}, weight_property => 'weight', max_hops => 1", vertex(1));
    let short = format!("'wg', {}, {}, 'weight'", vertex(1), vertex(2));

    // Act
    let states = [outcome(&world, &missing), outcome(&world, &short)];

    // Assert
    assert_eq!(states, ["42883", "42883"]);
}

#[test]
fn should_reject_more_than_nine_positional_arguments_with_42883() {
    // Arrange
    let world = world();
    let call = format!("'wg', {}, {}, 'weight', 1, 'out', NULL, NULL, 1, 7", vertex(1), vertex(2));

    // Act
    let result = outcome(&world, &call);

    // Assert
    assert_eq!(result, "42883");
}
