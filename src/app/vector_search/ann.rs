use std::mem::size_of;

use super::accounting::{check_controls, checked_mul, AccountedCandidates, AccountedVectorResult};
use super::{
    Cassie, CassieError, DistanceMetric, Instant, ProjectedVectorSearch, VectorIndexRecord,
};

impl Cassie {
    pub(super) fn try_hnsw_vector_search_controlled(
        &self,
        index: &VectorIndexRecord,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<Option<AccountedVectorResult>, CassieError> {
        if index.metadata.metric != request.metric {
            self.runtime.record_hnsw_fallback("incompatible-metric");
            return Ok(None);
        }
        if index.metadata.hnsw.is_none() {
            self.runtime.record_hnsw_fallback("missing-options");
            return Ok(None);
        }
        let started_at = Instant::now();
        let batch = match self.midge.search_hnsw_graph_point_read_controlled(
            request.collection,
            request.vector_field,
            request.query,
            &index.metadata,
            request.top_needed,
            request.controls,
        ) {
            Ok(Some(batch)) => batch,
            Ok(None) => {
                self.runtime.record_hnsw_fallback("missing-graph");
                return Ok(None);
            }
            Err(error) => return self.rest_ann_fallback(error, true),
        };
        let source_rows = batch.source_row_count;
        let (_, candidates, candidate_count, _, _candidate_memory) = batch.into_parts();
        if candidates.len() < request.top_needed.min(source_rows) {
            self.runtime.record_hnsw_fallback("insufficient-candidates");
            return Ok(None);
        }
        let selected = candidates
            .into_iter()
            .skip(request.offset)
            .take(request.limit)
            .map(|candidate| candidate.id);
        let mut result = self.vector_result_for_ids(request, selected)?;
        result.diagnostics.execution = Some((
            started_at.elapsed(),
            candidate_count,
            result.result.rows.len(),
        ));
        result.diagnostics.hnsw = Some(candidate_count);
        Ok(Some(result))
    }

    pub(super) fn try_ivfflat_vector_search_controlled(
        &self,
        request: &ProjectedVectorSearch<'_>,
    ) -> Result<Option<AccountedVectorResult>, CassieError> {
        if request.metric != DistanceMetric::L2 {
            self.runtime.record_ivfflat_fallback("incompatible-metric");
            return Ok(None);
        }
        let started_at = Instant::now();
        let snapshot = match self.midge.get_ivfflat_training_manifest_controlled(
            request.collection,
            request.vector_field,
            request.controls,
        ) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) => {
                self.runtime.record_ivfflat_fallback("missing-training");
                return Ok(None);
            }
            Err(error) => return self.rest_ann_fallback(error, false),
        };
        let (_, training, membership_count, _, _manifest_memory) = snapshot.into_parts();
        if let Some(reason) = crate::vector::ivfflat::compact_manifest_fallback_reason(
            &training,
            request.query.len(),
            membership_count,
        ) {
            self.runtime.record_ivfflat_fallback(reason);
            return Ok(None);
        }
        let _query_memory = request
            .controls
            .reserve_query_memory(checked_mul(request.query.len(), size_of::<f32>())?)?;
        let normalized_query = super::normalize_vector(request.query)
            .map_or_else(|| request.query.to_vec(), |normalized| normalized.values);
        let probe_bytes = super::checked_add(
            checked_mul(training.centroids.len(), 2 * size_of::<(usize, f64)>())?,
            checked_mul(
                training.probes.min(training.lists),
                size_of::<usize>() + 4 * size_of::<usize>(),
            )?,
        )?;
        let _probe_memory = request.controls.reserve_query_memory(probe_bytes)?;
        let probed_lists = crate::vector::ivfflat::probe_lists(&normalized_query, &training);
        let batch = match self.midge.ivfflat_candidate_vectors_controlled(
            request.collection,
            request.vector_field,
            &training,
            &probed_lists,
            request.controls,
        ) {
            Ok(batch) => batch,
            Err(error) => return self.rest_ann_fallback(error, false),
        };
        let (_, records, _, _, _candidate_memory) = batch.into_parts();
        let mut top = AccountedCandidates::new(request.controls)?;
        let mut candidate_count = 0usize;
        for record in records {
            check_controls(request.controls)?;
            let _vector_memory = request
                .controls
                .reserve_query_memory(checked_mul(record.values.len(), size_of::<f32>())?)?;
            let Some(vector) = crate::vector::ivfflat::denormalized_vector(&record) else {
                continue;
            };
            let distance =
                super::vector_distance_for_metric(request.metric, request.query, &vector);
            top.push(distance, &record.id, request.top_needed)?;
            candidate_count = candidate_count.saturating_add(1);
        }
        if candidate_count < request.top_needed.min(membership_count) {
            self.runtime
                .record_ivfflat_fallback("insufficient-candidates");
            return Ok(None);
        }
        if candidate_count == 0 {
            self.runtime.record_ivfflat_fallback("empty-probed-lists");
            return Ok(None);
        }
        let (ranked, _top_memory) = top.ranked();
        let selected = ranked
            .into_iter()
            .skip(request.offset)
            .take(request.limit)
            .map(|candidate| candidate.id);
        let mut result = self.vector_result_for_ids(request, selected)?;
        result.diagnostics.execution = Some((
            started_at.elapsed(),
            candidate_count,
            result.result.rows.len(),
        ));
        result.diagnostics.ivfflat = Some((training.lists, probed_lists.len(), candidate_count));
        Ok(Some(result))
    }

    fn rest_ann_fallback(
        &self,
        error: CassieError,
        hnsw: bool,
    ) -> Result<Option<AccountedVectorResult>, CassieError> {
        let message = error.to_string();
        let prefix = if hnsw {
            "hnsw fallback:"
        } else {
            "ivfflat fallback:"
        };
        if let Some((_, reason)) = message.split_once(prefix) {
            if hnsw {
                self.runtime.record_hnsw_fallback(reason);
            } else {
                self.runtime.record_ivfflat_fallback(reason);
            }
            return Ok(None);
        }
        Err(error)
    }
}
