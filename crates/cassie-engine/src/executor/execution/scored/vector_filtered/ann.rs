//! Adaptive ANN windows for filtered vector search.
//!
//! The ANN index knows nothing about the scalar filter, so a window of nearest candidates may
//! contain no eligible row at all. Each window is therefore grown (HNSW: a larger search limit;
//! `IVFFlat`: more probed lists) until enough eligible candidates are collected, the index is
//! exhausted, or the declared candidate cap is reached. The caller exact-reranks the eligible
//! candidates from source rows and never trusts an underfilled window.

use std::collections::HashSet;

use super::super::QueryError;
use super::{ANN_RERANK_OVERSAMPLE, INITIAL_WINDOW_FLOOR, INITIAL_WINDOW_PER_ROW};
use crate::app::{Cassie, CassieError};
use crate::embeddings::{DistanceMetric, VectorIndexRecord, VectorIndexType};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(in super::super) struct AnnRequest<'a> {
    pub(in super::super) cassie: &'a Cassie,
    pub(in super::super) collection: &'a str,
    pub(in super::super) stored_field: &'a str,
    pub(in super::super) query: &'a [f32],
    pub(in super::super) metric: DistanceMetric,
    pub(in super::super) index: &'a VectorIndexRecord,
    pub(in super::super) subset: &'a HashSet<String>,
    pub(in super::super) top_needed: usize,
    pub(in super::super) controls: &'a QueryExecutionControls,
}

/// Eligible candidates selected by the adaptive windows.
pub(in super::super) struct AnnEligible {
    pub(in super::super) ids: Vec<String>,
    pub(in super::super) generation: u64,
    pub(in super::super) windows: usize,
    pub(in super::super) candidates_read: usize,
    pub(in super::super) ann_reads: usize,
    /// `IVFFlat` lists in the index and lists probed by the final window (zero for HNSW).
    pub(in super::super) lists: (usize, usize),
    pub(in super::super) memory: QueryMemoryReservation,
}

pub(in super::super) enum AnnOutcome {
    Eligible(AnnEligible),
    /// The index could not serve the request; the label is the fallback reason.
    Declined(String),
}

impl AnnRequest<'_> {
    /// Eligible candidates wanted before the window stops growing.
    fn target(&self) -> usize {
        self.subset
            .len()
            .min(self.top_needed.saturating_mul(ANN_RERANK_OVERSAMPLE))
            .max(1)
    }

    fn candidate_cap(&self) -> usize {
        self.cassie
            .runtime
            .limits()
            .adaptive_candidate_max
            .max(self.top_needed)
            .max(1)
    }
}

pub(in super::super) fn eligible_ann_candidates(
    request: &AnnRequest<'_>,
) -> Result<AnnOutcome, QueryError> {
    match request.index.metadata.index_type {
        VectorIndexType::Hnsw => hnsw_windows(request),
        VectorIndexType::IvfFlat => ivfflat_windows(request),
        VectorIndexType::BruteForce => Ok(AnnOutcome::Declined("no-ann-index".to_string())),
    }
}

fn reserve_ids(
    controls: &QueryExecutionControls,
    ids: &[String],
) -> Result<QueryMemoryReservation, QueryError> {
    let bytes = ids.iter().fold(
        ids.len().saturating_mul(std::mem::size_of::<String>()),
        |bytes, id| bytes.saturating_add(id.len()),
    );
    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

fn storage_decline(error: CassieError, marker: &str) -> Result<AnnOutcome, QueryError> {
    let message = error.to_string();
    if let Some((_, reason)) = message.split_once(marker) {
        return Ok(AnnOutcome::Declined(reason.to_string()));
    }
    Err(QueryError::from(error))
}

fn hnsw_windows(request: &AnnRequest<'_>) -> Result<AnnOutcome, QueryError> {
    let cap = request.candidate_cap();
    let target = request.target();
    let mut window = request
        .top_needed
        .saturating_mul(INITIAL_WINDOW_PER_ROW)
        .max(INITIAL_WINDOW_FLOOR)
        .min(cap);
    let (mut windows, mut candidates_read, mut ann_reads) = (0usize, 0usize, 0usize);
    let mut last_returned = 0usize;
    loop {
        super::super::super::check_timeout(request.controls)?;
        let batch = match request
            .cassie
            .midge
            .search_hnsw_graph_point_read_controlled(
                request.collection,
                request.stored_field,
                request.query,
                &request.index.metadata,
                window,
                &request.controls.storage_request(),
            ) {
            Ok(Some(batch)) => batch,
            Ok(None) => return Ok(AnnOutcome::Declined("missing-graph".to_string())),
            Err(error) => return storage_decline(error.into(), "hnsw fallback:"),
        };
        let source_rows = batch.source_row_count();
        let (generation, candidates, _, reads, candidate_memory) = batch.into_parts();
        windows += 1;
        ann_reads = ann_reads.saturating_add(reads);
        candidates_read = candidates_read.saturating_add(candidates.len());
        let returned = candidates.len();
        let mut candidates = candidates;
        candidates.sort_by(|left, right| {
            left.distance
                .total_cmp(&right.distance)
                .then_with(|| left.id.cmp(&right.id))
        });
        let mut ids = Vec::new();
        for candidate in candidates {
            if request.subset.contains(&candidate.id) {
                ids.push(candidate.id);
                if ids.len() >= target {
                    break;
                }
            }
        }
        drop(candidate_memory);
        // Only a window that returned every indexed row proves the index is exhausted. A search
        // that stops returning new rows is limited by graph reach, and the eligible rows it
        // missed cannot be distinguished from absent ones.
        let exhausted = returned >= source_rows;
        if ids.len() >= target || exhausted {
            let memory = reserve_ids(request.controls, &ids)?;
            return Ok(AnnOutcome::Eligible(AnnEligible {
                ids,
                generation,
                windows,
                candidates_read,
                ann_reads,
                lists: (0, 0),
                memory,
            }));
        }
        if returned < window && returned <= last_returned {
            return Ok(AnnOutcome::Declined("graph-reach-limited".to_string()));
        }
        last_returned = returned;
        if window >= cap {
            return Ok(AnnOutcome::Declined("candidate-cap".to_string()));
        }
        window = window.saturating_mul(2).min(cap);
    }
}

fn ivfflat_windows(request: &AnnRequest<'_>) -> Result<AnnOutcome, QueryError> {
    let cap = request.candidate_cap();
    let target = request.target();
    let Some(snapshot) = request
        .cassie
        .midge
        .get_ivfflat_training_manifest_controlled(
            request.collection,
            request.stored_field,
            &request.controls.storage_request(),
        )?
    else {
        return Ok(AnnOutcome::Declined("missing-training".to_string()));
    };
    let (generation, mut training, membership_count, manifest_reads, manifest_memory) =
        snapshot.into_parts();
    if let Some(reason) = crate::vector::ivfflat::compact_manifest_fallback_reason(
        &training,
        request.query.len(),
        membership_count,
    ) {
        return Ok(AnnOutcome::Declined(reason.to_string()));
    }
    let normalized = crate::vector::normalize(request.query)
        .map_or_else(|| request.query.to_vec(), |normalized| normalized.values);
    let total_lists = training.lists.max(1);
    let mut probes = training.probes.max(1).min(total_lists);
    let (mut windows, mut candidates_read) = (0usize, 0usize);
    let mut ann_reads = manifest_reads;
    loop {
        super::super::super::check_timeout(request.controls)?;
        training.probes = probes;
        let lists = if request.metric == DistanceMetric::Dot {
            let Some(lists) =
                crate::vector::ivfflat::probe_lists_by_inner_product(request.query, &training)
            else {
                return Ok(AnnOutcome::Declined("missing-list-bounds".to_string()));
            };
            lists
        } else {
            crate::vector::ivfflat::probe_lists(&normalized, &training)
        };
        let _list_memory = request
            .controls
            .reserve_query_memory(lists.len().saturating_mul(std::mem::size_of::<usize>()))?;
        let batch = match request.cassie.midge.ivfflat_candidate_vectors_controlled(
            request.collection,
            request.stored_field,
            &training,
            &lists,
            &request.controls.storage_request(),
        ) {
            Ok(batch) => batch,
            Err(error) => return storage_decline(error.into(), "ivfflat fallback:"),
        };
        let (batch_generation, records, membership_reads, vector_reads, candidate_memory) =
            batch.into_parts();
        if batch_generation != generation {
            return Ok(AnnOutcome::Declined("concurrent-source-change".to_string()));
        }
        windows += 1;
        ann_reads = ann_reads
            .saturating_add(1)
            .saturating_add(membership_reads)
            .saturating_add(vector_reads);
        candidates_read = candidates_read.saturating_add(records.len());
        let ids: Vec<String> = records
            .into_iter()
            .filter(|record| request.subset.contains(&record.id))
            .map(|record| record.id)
            .take(cap)
            .collect();
        drop(candidate_memory);
        let exhausted = probes >= total_lists;
        if ids.len() >= target || exhausted {
            drop(manifest_memory);
            let memory = reserve_ids(request.controls, &ids)?;
            return Ok(AnnOutcome::Eligible(AnnEligible {
                ids,
                generation,
                windows,
                candidates_read,
                ann_reads,
                lists: (total_lists, probes),
                memory,
            }));
        }
        probes = probes.saturating_mul(2).min(total_lists);
    }
}
