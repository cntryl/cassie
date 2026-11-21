//! One test per guide world: every example of the world runs in document order.

use crate::guide;
use crate::runner::run_world;

fn run(world: &str) -> Vec<String> {
    run_world(world, &guide::parse().examples)
}

#[test]
fn should_run_every_definition_example() {
    // Arrange
    let world = "define";

    // Act
    let failures = run(world);

    // Assert
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn should_run_every_social_example() {
    // Arrange
    let world = "social";

    // Act
    let failures = run(world);

    // Assert
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn should_run_every_authorization_example() {
    // Arrange
    let world = "access";

    // Act
    let failures = run(world);

    // Assert
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn should_run_every_hierarchy_example() {
    // Arrange
    let world = "org";

    // Act
    let failures = run(world);

    // Assert
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn should_run_every_weighted_route_example() {
    // Arrange
    let world = "routes";

    // Act
    let failures = run(world);

    // Assert
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn should_run_every_lifecycle_example() {
    // Arrange
    let world = "lifecycle";

    // Act
    let failures = run(world);

    // Assert
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn should_run_every_permission_example() {
    // Arrange
    let world = "permissions";

    // Act
    let failures = run(world);

    // Assert
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn should_run_every_legacy_removal_example() {
    // Arrange
    let world = "legacy";

    // Act
    let failures = run(world);

    // Assert
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn should_run_every_stable_identity_example() {
    // Arrange
    let world = "identity";

    // Act
    let failures = run(world);

    // Assert
    assert_eq!(failures, Vec::<String>::new());
}

#[test]
fn should_run_every_database_clone_example() {
    // Arrange
    let world = "clone";

    // Act
    let failures = run(world);

    // Assert
    assert_eq!(failures, Vec::<String>::new());
}
