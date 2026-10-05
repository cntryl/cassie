use std::mem::size_of;

use crate::app::CassieError;

/// Charge a complete `BTree` node for each object entry, including its unused slots.
///
/// Rust's `BTree` nodes contain eleven key/value slots. Sixteen pointer-sized words cover
/// the parent/header, alignment and twelve child edges of an internal node. Charging a
/// full internal node per entry also bounds sparse, independently allocated leaf maps.
pub(crate) const JSON_MAP_ENTRY_BYTES: usize =
    11 * (size_of::<String>() + size_of::<serde_json::Value>()) + 16 * size_of::<usize>();

/// Conservative serialized-JSON decode bound, separate from owned-value accounting.
///
/// A nested empty-key object adds at least five serialized bytes per occupied node.
/// On supported 32/64-bit targets, 192 bytes per serialized byte covers the node bound,
/// value/array slots, decoded key/string buffers and transient decode copies. This
/// factor must be requalified if `serde_json`'s map backend or Rust's node layout changes.
pub(crate) const JSON_DECODE_EXPANSION_FACTOR: usize = 192;

/// Estimates an already-owned JSON value, including unused String and Vec capacity.
///
/// The estimate can also reserve a clone before construction: source capacities bound
/// the clone's lengths, and node charges include the map's inline keys and child values.
/// Empty owned maps receive one node allowance because removing their final entry can
/// retain the root allocation. Their public API does not expose that allocation state.
pub(crate) fn retained_bytes(value: &serde_json::Value) -> Result<usize, CassieError> {
    checked_add(
        size_of::<serde_json::Value>(),
        owned_heap_bytes(value, true)?,
    )
}

/// Estimates fresh parser/reconstruction output whose empty maps were never populated.
///
/// Use this only immediately after decoding raw storage values or constructing a new
/// projected map from those decoded values. Such empty maps have no retained `BTree` root.
/// Arbitrary owned values, including staged state, must use `retained_bytes` instead.
pub(crate) fn retained_decoded_bytes(value: &serde_json::Value) -> Result<usize, CassieError> {
    checked_add(
        size_of::<serde_json::Value>(),
        owned_heap_bytes(value, false)?,
    )
}

pub(crate) fn serialized_decode_bytes(raw_bytes: usize) -> Result<usize, CassieError> {
    checked_mul(raw_bytes, JSON_DECODE_EXPANSION_FACTOR)
}

fn owned_heap_bytes(
    value: &serde_json::Value,
    charge_empty_roots: bool,
) -> Result<usize, CassieError> {
    match value {
        serde_json::Value::String(text) => Ok(text.capacity()),
        serde_json::Value::Array(values) => values.iter().try_fold(
            checked_mul(values.capacity(), size_of::<serde_json::Value>())?,
            |bytes, value| checked_add(bytes, owned_heap_bytes(value, charge_empty_roots)?),
        ),
        serde_json::Value::Object(values) => values.iter().try_fold(
            checked_mul(
                if charge_empty_roots {
                    values.len().max(1)
                } else {
                    values.len()
                },
                JSON_MAP_ENTRY_BYTES,
            )?,
            |bytes, (key, value)| {
                checked_add(
                    bytes,
                    checked_add(key.capacity(), owned_heap_bytes(value, charge_empty_roots)?)?,
                )
            },
        ),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            Ok(0)
        }
    }
}

fn checked_add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(accounting_overflow)
}

fn checked_mul(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_mul(right).ok_or_else(accounting_overflow)
}

fn accounting_overflow() -> CassieError {
    CassieError::ResourceLimit("retained JSON accounting overflow".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{
        retained_bytes, retained_decoded_bytes, serialized_decode_bytes, JSON_MAP_ENTRY_BYTES,
    };

    #[test]
    fn should_keep_freshly_decoded_empty_object_accounting_at_its_inline_size() {
        // Arrange
        let value: serde_json::Value = serde_json::from_str("{}").expect("fresh empty object");

        // Act
        let retained = retained_decoded_bytes(&value).expect("fresh decode estimate");

        // Assert
        assert_eq!(retained, std::mem::size_of::<serde_json::Value>());
        assert!(
            retained_bytes(&value).expect("unknown provenance estimate") >= JSON_MAP_ENTRY_BYTES
        );
    }

    #[test]
    fn should_include_a_retained_empty_object_node_after_removing_the_last_entry() {
        // Arrange
        let mut map = serde_json::Map::new();
        map.insert("one".to_owned(), serde_json::Value::Null);
        map.remove("one");
        let value = serde_json::Value::Object(map);

        // Act
        let retained = retained_bytes(&value).expect("owned empty object estimate");

        // Assert
        assert!(
            retained >= JSON_MAP_ENTRY_BYTES,
            "empty maps can retain an allocated BTree root"
        );
    }

    #[test]
    fn should_include_sparse_object_nodes_in_retained_json_memory() {
        // Arrange
        let mut nested = serde_json::json!(0);
        for _ in 0..32 {
            nested = serde_json::json!({"a": nested});
        }

        // Act
        let retained = retained_bytes(&nested).expect("JSON estimate");

        // Assert
        assert!(retained >= 32 * JSON_MAP_ENTRY_BYTES);
        assert!(retained > 16 * 1_024);
    }

    #[test]
    fn should_include_unused_buffer_capacity_in_owned_json_memory() {
        // Arrange
        let mut text = String::with_capacity(1_024);
        text.push('x');
        let mut values = Vec::with_capacity(128);
        values.push(serde_json::Value::String(text));
        let value = serde_json::Value::Array(values);

        // Act
        let retained = retained_bytes(&value).expect("JSON estimate");

        // Assert
        assert!(retained >= 128 * std::mem::size_of::<serde_json::Value>() + 1_024);
    }

    #[test]
    fn should_reject_serialized_json_accounting_overflow() {
        // Arrange
        let serialized_bytes = usize::MAX;

        // Act
        let result = serialized_decode_bytes(serialized_bytes);

        // Assert
        assert!(result.is_err());
    }
}
