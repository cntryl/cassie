use super::{Cassie, QueryError, VectorDistanceTopKSpec};

type AnnRerankBarriers = (
    std::sync::Arc<std::sync::Barrier>,
    std::sync::Arc<std::sync::Barrier>,
);

// The weak storage owner scopes the pause to the installing engine; a query
// against any other engine in the process passes the boundary untouched.
type InstalledAnnRerankBarriers = (std::sync::Weak<crate::midge::Midge>, AnnRerankBarriers);

static ANN_RERANK_BARRIERS: std::sync::Mutex<Option<InstalledAnnRerankBarriers>> =
    std::sync::Mutex::new(None);

pub(crate) fn install_ann_rerank_barriers(cassie: &Cassie, barriers: Option<AnnRerankBarriers>) {
    *ANN_RERANK_BARRIERS.lock().expect("ANN rerank barrier lock") =
        barriers.map(|barriers| (std::sync::Arc::downgrade(&cassie.midge), barriers));
}

pub(super) fn wait_at_ann_rerank_boundary(cassie: &Cassie) {
    let barriers = {
        let mut installed = ANN_RERANK_BARRIERS.lock().expect("ANN rerank barrier lock");
        if installed.as_ref().is_some_and(|(owner, _)| {
            std::ptr::eq(owner.as_ptr(), std::sync::Arc::as_ptr(&cassie.midge))
        }) {
            installed.take().map(|(_, barriers)| barriers)
        } else {
            None
        }
    };
    if let Some((selected, resume)) = barriers {
        selected.wait();
        resume.wait();
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
    stored_vector_field: &str,
) -> Result<(), QueryError> {
    record_ann_exact_fallback(
        cassie,
        spec,
        stored_vector_field,
        "transaction-overlay-exact",
    )
}

pub(super) fn record_filtered_ann_exact_fallback(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    stored_vector_field: &str,
) -> Result<(), QueryError> {
    record_ann_exact_fallback(cassie, spec, stored_vector_field, "structured-filter-exact")
}

fn record_ann_exact_fallback(
    cassie: &Cassie,
    spec: &VectorDistanceTopKSpec,
    stored_vector_field: &str,
    reason: &str,
) -> Result<(), QueryError> {
    let index = cassie
        .midge
        .get_vector_index_definition(&spec.collection, stored_vector_field)
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
