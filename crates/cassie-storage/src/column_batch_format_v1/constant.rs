use super::{scalar_bytes, LogicalType};
use crate::StorageCodecError;

pub(super) fn constant_eligible(
    logical_type: LogicalType,
    values: &[&serde_json::Value],
) -> Result<bool, StorageCodecError> {
    let Some(first) = values.first() else {
        return Ok(true);
    };
    let first = scalar_bytes(logical_type, first)?;
    for value in values.iter().skip(1) {
        if scalar_bytes(logical_type, value)? != first {
            return Ok(false);
        }
    }
    Ok(true)
}
