//! Operator diagnostics carry table-derived content (row counts, content
//! digests, mismatch samples), so a restricted role sees none of their rows.

use crate::sequence_state_privileges::{fixture, scalar};
use cassie::types::Value;

const HASH_ROWS: &str = "SELECT count(*) FROM pg_catalog.pg_projection_hashes";

#[test]
fn should_show_projection_hashes_to_an_administrator() {
    // Arrange
    let (cassie, root, _alice) = fixture(&[]);

    // Act
    let count = scalar(&cassie, &root, HASH_ROWS);

    // Assert
    assert!(matches!(count, Value::Int64(rows) if rows > 0));
}

#[test]
fn should_hide_projection_hashes_from_a_role_without_access() {
    // Arrange
    let (cassie, _root, alice) = fixture(&[]);

    // Act
    let count = scalar(&cassie, &alice, HASH_ROWS);

    // Assert
    assert_eq!(count, Value::Int64(0));
}
