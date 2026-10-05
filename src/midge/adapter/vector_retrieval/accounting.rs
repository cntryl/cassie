use std::mem::size_of;

use crate::app::CassieError;

use super::super::codec::PersistedVectorIndexState;
use super::HnswSourceSummary;

/// Bounds raw state and the binary decoder's simultaneously live owned buffers.
///
/// Strings have four-byte lengths and eight-byte container counts; vectors have
/// eight-byte counts and four-byte f32 elements. Sixteen bytes per serialized byte
/// cover String/Vec slots, sparse initial capacities, growth and relocation slack
/// on supported 32/64-bit targets. Requalify this bound when the decoder or Vec
/// allocation strategy changes. Inline decoded state is charged separately.
pub(super) fn vector_state_bytes(raw_bytes: usize) -> Result<usize, CassieError> {
    raw_bytes
        .checked_mul(16)
        .and_then(|bytes| bytes.checked_add(size_of::<PersistedVectorIndexState>()))
        .ok_or_else(accounting_overflow)
}

pub(super) fn source_summary_bytes(raw_bytes: usize) -> Result<usize, CassieError> {
    raw_bytes
        .checked_add(size_of::<HnswSourceSummary>())
        .ok_or_else(accounting_overflow)
}

pub(super) fn ivfflat_membership_bytes(key: &[u8], value: &[u8]) -> Result<usize, CassieError> {
    // One complete internal BTree node per ID bounds sparse leaves and unused slots.
    let node_bytes = 11 * size_of::<String>() + 16 * size_of::<usize>();
    // The encoded key bounds the decoded ID length. Keep both owned ID copies and
    // the raw entry covered before decoding, BTree insertion and exact Vec growth.
    key.len()
        .checked_mul(3)
        .and_then(|bytes| bytes.checked_add(value.len()))
        .and_then(|bytes| bytes.checked_add(node_bytes))
        .and_then(|bytes| bytes.checked_add(size_of::<String>()))
        .ok_or_else(accounting_overflow)
}

fn accounting_overflow() -> CassieError {
    CassieError::ResourceLimit("controlled vector metadata accounting overflow".to_owned())
}
