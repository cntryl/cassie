use super::{Cassie, QueryError, VectorDistanceTopKSpec};

type AnnRerankBarriers = (
    std::sync::Arc<std::sync::Barrier>,
    std::sync::Arc<std::sync::Barrier>,
);

// The weak storage owner scopes the pause to the installing engine; a query
// against any other engine in the process passes the boundary untouched.
type InstalledAnnRerankBarriers = (std::sync::Weak<crate::midge::Midge>, AnnRerankBarriers);

static ANN_RERANK_BARRIERS: std::sync::Mutex<Vec<InstalledAnnRerankBarriers>> =
    std::sync::Mutex::new(Vec::new());

pub(crate) fn install_ann_rerank_barriers(cassie: &Cassie, barriers: Option<AnnRerankBarriers>) {
    let owner = std::sync::Arc::as_ptr(&cassie.midge);
    let mut installed = ANN_RERANK_BARRIERS.lock().expect("ANN rerank barrier lock");
    installed.retain(|(installed_owner, _)| {
        installed_owner.strong_count() > 0 && !std::ptr::eq(installed_owner.as_ptr(), owner)
    });
    if let Some(barriers) = barriers {
        installed.push((std::sync::Arc::downgrade(&cassie.midge), barriers));
    }
}

pub(super) fn wait_at_ann_rerank_boundary(cassie: &Cassie) {
    let barriers = {
        let mut installed = ANN_RERANK_BARRIERS.lock().expect("ANN rerank barrier lock");
        let owner = std::sync::Arc::as_ptr(&cassie.midge);
        installed
            .iter()
            .position(|(installed_owner, _)| std::ptr::eq(installed_owner.as_ptr(), owner))
            .map(|index| installed.swap_remove(index).1)
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

#[cfg(test)]
mod tests {
    use super::{install_ann_rerank_barriers, ANN_RERANK_BARRIERS};
    use crate::app::Cassie;
    use crate::config::CassieRuntimeConfig;
    use std::sync::{Arc, Barrier};

    #[test]
    fn should_keep_ann_rerank_barriers_isolated_by_engine() {
        // Arrange
        let first = Arc::new(
            Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
                .expect("first in-memory Cassie"),
        );
        let second = Arc::new(
            Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
                .expect("second in-memory Cassie"),
        );
        let barrier_pair = || (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));

        // Act
        install_ann_rerank_barriers(&first, Some(barrier_pair()));
        install_ann_rerank_barriers(&second, Some(barrier_pair()));
        let installed = ANN_RERANK_BARRIERS.lock().expect("ANN rerank barrier lock");
        let first_registered = installed
            .iter()
            .any(|(owner, _)| std::ptr::eq(owner.as_ptr(), Arc::as_ptr(&first.midge)));
        let second_registered = installed
            .iter()
            .any(|(owner, _)| std::ptr::eq(owner.as_ptr(), Arc::as_ptr(&second.midge)));
        drop(installed);
        install_ann_rerank_barriers(&first, None);
        install_ann_rerank_barriers(&second, None);

        // Assert
        assert!(first_registered);
        assert!(second_registered);
    }
}
