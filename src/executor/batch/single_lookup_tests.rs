use super::{BatchRow, DataType, Value};
use std::sync::Arc;

#[test]
fn should_resolve_exact_single_entry_without_initializing_lookup() {
    // Arrange
    for name in ["plain", "t.qualified", "\"Mixed Case\"", "\0invalid"] {
        let mut row = BatchRow::from_projected_values(vec![(name.into(), Value::Int64(7))]);
        row.set_data_types(vec![DataType::BigInt].into());
        // Act / Assert
        assert_eq!(row.get(name), Some(&Value::Int64(7)), "{name:?}");
        assert_eq!(row.column_type(name), Some(&DataType::BigInt), "{name:?}");
        assert!(!row.lookup_initialized(), "{name:?}");
        assert!(row.get("absent").is_none());
        assert!(row.lookup_initialized());
        assert_eq!(row.get(name), Some(&Value::Int64(7)));
        assert_eq!(row.column_type(name), Some(&DataType::BigInt));
    }
}

#[test]
fn should_preserve_missing_type_and_outer_scope_precedence_on_exact_match() {
    // Arrange
    let mut outer = BatchRow::new(vec![("t.a".into(), Value::Int64(1))]);
    outer.set_data_types(vec![DataType::BigInt].into());
    let row = BatchRow::from_projected_values(vec![("t.a".into(), Value::Int64(2))])
        .with_outer_scope(Arc::new(outer));
    // Act / Assert
    assert_eq!(row.get("t.a"), Some(&Value::Int64(2)));
    assert_eq!(row.column_type("t.a"), None);
    assert!(!row.lookup_initialized());
}

#[test]
fn should_keep_alias_and_nonexact_qualified_lookups_on_the_existing_path() {
    // Arrange
    let mut alias = BatchRow::from_projected_values(vec![("value".into(), Value::Int64(1))]);
    alias.aliases.push(("alias".into(), 0));
    assert!(!alias.lookup_initialized());
    let outer = Arc::new(BatchRow::new(vec![("t.value".into(), Value::Int64(9))]));
    let inner = BatchRow::from_projected_values(vec![("value".into(), Value::Int64(1))])
        .with_outer_scope(outer);
    // Act / Assert
    assert_eq!(alias.get("value"), Some(&Value::Int64(1)));
    assert!(alias.lookup_initialized());
    assert_eq!(alias.get("alias"), Some(&Value::Int64(1)));
    assert_eq!(inner.get("t.value"), Some(&Value::Int64(9)));
    assert!(inner.lookup_initialized());
}

#[test]
fn should_leave_empty_and_multi_entry_rows_on_the_existing_lookup_path() {
    // Arrange
    let empty = BatchRow::from_projected_values(vec![]);
    let many = BatchRow::from_projected_values(vec![
        ("a".into(), Value::Int64(1)),
        ("b".into(), Value::Int64(2)),
    ]);
    // Act / Assert
    assert!(empty.get("a").is_none());
    assert!(empty.lookup_initialized());
    assert_eq!(many.get("a"), Some(&Value::Int64(1)));
    assert!(many.lookup_initialized());
}
