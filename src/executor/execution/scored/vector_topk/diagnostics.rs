use super::{Cassie, QueryError, VectorDistanceTopKSpec};

type AnnRerankBarriers = (
    std::sync::Arc<std::sync::Barrier>,
    std::sync::Arc<std::sync::Barrier>,
);

static ANN_RERANK_BARRIERS: std::sync::OnceLock<std::sync::Mutex<Option<AnnRerankBarriers>>> =
    std::sync::OnceLock::new();

pub(crate) fn install_ann_rerank_barriers(barriers: Option<AnnRerankBarriers>) {
    *ANN_RERANK_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("ANN rerank barrier lock") = barriers;
}

pub(super) fn wait_at_ann_rerank_boundary() {
    let barriers = ANN_RERANK_BARRIERS
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .expect("ANN rerank barrier lock")
        .clone();
    if let Some((selected, resume)) = barriers {
        selected.wait();
        resume.wait();
        install_ann_rerank_barriers(None);
    }
}

pub(super) fn source_generation_matches(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    expected: u64,
) -> Result<bool, QueryError> {
    cassie
        .midge
        .collection_generation(&spec.collection)
        .map(|current| current == expected)
        .map_err(QueryError::from)
}

pub(super) fn record_transaction_overlay_exact_fallback(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
) -> Result<(), QueryError> {
    record_ann_exact_fallback(cassie, spec, "transaction-overlay-exact")
}

pub(super) fn record_filtered_ann_exact_fallback(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
) -> Result<(), QueryError> {
    record_ann_exact_fallback(cassie, spec, "structured-filter-exact")
}

fn record_ann_exact_fallback(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    reason: &str,
) -> Result<(), QueryError> {
    let index = cassie
        .midge
        .get_vector_index_definition(&spec.collection, &spec.vector_field)
        .map_err(QueryError::from)?;
    match index.map(|record| record.metadata.index_type) {
        Some(crate::embeddings::VectorIndexType::Hnsw) => {
            cassie.runtime.record_hnsw_fallback(reason);
        }
        Some(crate::embeddings::VectorIndexType::IvfFlat) => {
            cassie.runtime.record_ivfflat_fallback(reason);
        }
        Some(crate::embeddings::VectorIndexType::BruteForce) | None => {}
    }
    Ok(())
}

pub(super) fn record_hnsw_concurrent_source_change(cassie: &Cassie) {
    cassie
        .runtime
        .record_hnsw_fallback("concurrent-source-change");
}

pub(super) fn record_ivfflat_concurrent_source_change(cassie: &Cassie) {
    cassie
        .runtime
        .record_ivfflat_fallback("concurrent-source-change");
}
