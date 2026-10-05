use std::mem::size_of;

use crate::app::CassieError;
use crate::embeddings::{HnswGraphNode, VectorIndexMetadata};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) fn validate_manifest(
    manifest: &super::super::codec::PersistedHnswManifest,
    metadata: &VectorIndexMetadata,
) -> Result<(), CassieError> {
    let reason = if manifest.version != crate::vector::hnsw::HNSW_GRAPH_VERSION {
        Some("unsupported-graph-version")
    } else if manifest.metric != metadata.metric {
        Some("incompatible-metric")
    } else if manifest.dimensions != metadata.dimensions {
        Some("incompatible-dimensions")
    } else {
        None
    };
    if let Some(reason) = reason {
        return Err(CassieError::Execution(format!("hnsw fallback:{reason}")));
    }
    Ok(())
}

/// Bounds raw bytes and lazy decoder growth without trusting stored counts.
///
/// Each decoded layer/neighbor requires a four-byte count/length in the compact frame.
/// Thirty-two bytes per raw byte cover raw storage, f32 buffers, the sparse Vec minimums,
/// growth slack and transient relocation buffers for those observed entries. The node's
/// owned identity is separate because it is supplied by the lookup key, not the frame.
const DECODE_EXPANSION_FACTOR: usize = 32;

pub(super) fn decode_node(
    raw: &[u8],
    id: &str,
    controls: &QueryExecutionControls,
) -> Result<(HnswGraphNode, QueryMemoryReservation), CassieError> {
    let bytes = raw
        .len()
        .checked_mul(DECODE_EXPANSION_FACTOR)
        .and_then(|bytes| bytes.checked_add(size_of::<HnswGraphNode>()))
        .and_then(|bytes| bytes.checked_add(id.len()))
        .ok_or_else(|| {
            CassieError::ResourceLimit("controlled HNSW node decode accounting overflow".to_owned())
        })?;
    let memory = controls.reserve_query_memory(bytes)?;
    let node = super::super::codec::decode_hnsw_node(raw, id).map_err(|error| match error {
        error @ CassieError::ResourceLimit(_) => error,
        _ => CassieError::Execution("hnsw fallback:invalid-node".to_owned()),
    })?;
    // Keep the decoded node and its reservation coupled until the traversal cache owns them.
    Ok((node, memory))
}
