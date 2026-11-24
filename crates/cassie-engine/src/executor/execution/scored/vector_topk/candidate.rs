use std::collections::BinaryHeap;

use crate::app::CassieError;
use crate::executor::batch::BatchRow;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

use super::{SortDirection, VectorDistanceTopKSpec};

pub(super) struct AccountedVectorTopK {
    top: BinaryHeap<SqlVectorCandidate>,
    memory: QueryMemoryReservation,
}

impl AccountedVectorTopK {
    pub(super) fn try_new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            top: BinaryHeap::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }

    pub(super) fn try_push(
        &mut self,
        candidate: SqlVectorCandidate,
        top_needed: usize,
    ) -> Result<(), CassieError> {
        if self.top.len() < top_needed {
            let previous_bytes = self.memory.bytes();
            self.memory.try_grow(candidate.retained_bytes())?;
            if let Err(error) = self.top.try_reserve_exact(1) {
                self.memory.shrink_to(previous_bytes);
                return Err(CassieError::ResourceLimit(format!(
                    "unable to retain vector top-k candidate: {error}"
                )));
            }
            self.top.push(candidate);
        } else if self
            .top
            .peek()
            .is_some_and(|worst| candidate.is_better_than(worst))
        {
            let previous_bytes = self
                .top
                .peek()
                .map_or(0, SqlVectorCandidate::retained_bytes);
            let next_bytes = candidate.retained_bytes();
            if next_bytes > previous_bytes {
                self.memory.try_grow(next_bytes - previous_bytes)?;
            }
            self.top.pop();
            self.top.push(candidate);
            if next_bytes < previous_bytes {
                self.memory
                    .shrink_to(self.memory.bytes() - (previous_bytes - next_bytes));
            }
        }
        Ok(())
    }

    pub(super) fn into_parts(self) -> (BinaryHeap<SqlVectorCandidate>, QueryMemoryReservation) {
        (self.top, self.memory)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SqlVectorCandidate {
    pub(super) sort_value: f64,
    pub(super) score: f64,
    pub(super) id: String,
}

impl SqlVectorCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_sql_vector_candidates(self, other) == std::cmp::Ordering::Less
    }

    fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>().saturating_add(self.id.len())
    }
}

impl Eq for SqlVectorCandidate {}

impl PartialOrd for SqlVectorCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SqlVectorCandidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        compare_sql_vector_candidates(self, other)
    }
}

pub(super) fn candidate_sort_value(direction: &SortDirection, score: f64) -> f64 {
    match direction {
        SortDirection::Asc => score,
        SortDirection::Desc => -score,
    }
}

pub(super) fn compare_sql_vector_candidates(
    left: &SqlVectorCandidate,
    right: &SqlVectorCandidate,
) -> std::cmp::Ordering {
    left.sort_value
        .total_cmp(&right.sort_value)
        .then_with(|| left.id.cmp(&right.id))
}

pub(super) fn vector_rows_from_top(
    top: BinaryHeap<SqlVectorCandidate>,
    spec: &VectorDistanceTopKSpec,
) -> Vec<BatchRow> {
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_sql_vector_candidates);
    vector_rows_from_ranked(ranked, spec)
}

pub(super) fn vector_rows_from_ranked(
    ranked: Vec<SqlVectorCandidate>,
    spec: &VectorDistanceTopKSpec,
) -> Vec<BatchRow> {
    ranked
        .into_iter()
        .skip(spec.offset)
        .take(spec.limit)
        .map(|candidate| {
            BatchRow::new(vec![
                (spec.id_column.clone(), Value::String(candidate.id)),
                (spec.score_column.clone(), Value::Float64(candidate.score)),
            ])
        })
        .collect()
}
