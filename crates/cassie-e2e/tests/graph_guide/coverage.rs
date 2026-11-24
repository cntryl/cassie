//! The guide and its tests cover each other: no SQL block is left unexecuted and no world is
//! left without a test.

use std::collections::BTreeSet;

use crate::guide;

const WORLDS: [&str; 10] = [
    "identity",
    "define",
    "social",
    "access",
    "org",
    "routes",
    "lifecycle",
    "permissions",
    "legacy",
    "clone",
];

#[test]
fn should_tag_every_sql_block_as_an_example() {
    // Arrange
    let parsed = guide::parse();

    // Act
    let untagged = parsed.untagged_blocks;

    // Assert
    assert_eq!(untagged, 0);
}

#[test]
fn should_give_every_example_a_unique_id() {
    // Arrange
    let parsed = guide::parse();

    // Act
    let ids = parsed
        .examples
        .iter()
        .map(|example| example.id.as_str())
        .collect::<BTreeSet<_>>();

    // Assert
    assert_eq!(ids.len(), parsed.examples.len());
}

#[test]
fn should_place_every_example_in_a_world_with_a_test() {
    // Arrange
    let parsed = guide::parse();

    // Act
    let worlds = parsed
        .examples
        .iter()
        .map(|example| example.world.as_str())
        .collect::<BTreeSet<_>>();

    // Assert
    assert_eq!(worlds, BTreeSet::from(WORLDS));
}
