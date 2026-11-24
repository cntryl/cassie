//! Row id admission for writers.

use crate::adapter::CassieError;

/// Rejects a row id holding a NUL byte.
///
/// Row ids are the last component of a composite key whose components are
/// joined by NUL, so such an id would be read back under a different id.
/// PostgreSQL text cannot hold NUL either; the SQLSTATE matches (22021).
///
/// # Errors
///
/// Returns `InvalidQuery` (SQLSTATE 22021) when `id` contains NUL.
pub(super) fn reject_nul_row_id(id: &str) -> Result<(), CassieError> {
    if id.contains('\0') {
        return Err(CassieError::InvalidQuery(
            "invalid byte sequence for encoding \"UTF8\": 0x00 in row id".to_owned(),
        ));
    }
    Ok(())
}
