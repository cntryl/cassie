use std::collections::BinaryHeap;
use std::mem::size_of;

use crate::app::CassieError;
use crate::executor::{ColumnMeta, QueryResult};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

use super::{compare_scored_vector_candidates, ScoredVectorCandidate};

pub(super) struct AccountedVectorResult {
    pub(super) result: QueryResult,
    pub(super) memory: QueryMemoryReservation,
    pub(super) row_memory: Option<QueryMemoryReservation>,
    pub(super) diagnostics: VectorSearchDiagnostics,
}

#[derive(Default)]
pub(super) struct VectorSearchDiagnostics {
    pub(super) normalization: Option<(usize, usize)>,
    pub(super) execution: Option<(std::time::Duration, usize, usize)>,
    pub(super) hnsw: Option<usize>,
    pub(super) ivfflat: Option<(usize, usize, usize)>,
}

impl VectorSearchDiagnostics {
    pub(super) fn record_success(self, cassie: &crate::app::Cassie) {
        if let Some((normalized, fallback)) = self.normalization {
            cassie
                .runtime
                .record_vector_normalization_usage(normalized, fallback);
        }
        if let Some((elapsed, candidates, rows)) = self.execution {
            cassie
                .runtime
                .record_vector_execution(elapsed, candidates, rows);
        }
        if let Some(candidates) = self.hnsw {
            cassie.runtime.record_hnsw_execution(candidates);
        }
        if let Some((lists, probes, candidates)) = self.ivfflat {
            cassie
                .runtime
                .record_ivfflat_execution(lists, probes, candidates);
        }
    }
}

pub(super) struct AccountedCandidates {
    top: BinaryHeap<ScoredVectorCandidate>,
    memory: QueryMemoryReservation,
}

impl AccountedCandidates {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            top: BinaryHeap::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }

    pub(super) fn push(
        &mut self,
        distance: f64,
        id: &str,
        top_needed: usize,
    ) -> Result<(), CassieError> {
        let candidate_bytes = checked_add(size_of::<ScoredVectorCandidate>(), id.len())?;
        if self.top.len() < top_needed {
            let previous = self.memory.bytes();
            self.memory.try_grow(candidate_bytes)?;
            if let Err(error) = self.top.try_reserve_exact(1) {
                self.memory.shrink_to(previous);
                return Err(CassieError::ResourceLimit(format!(
                    "unable to retain REST vector candidate: {error}"
                )));
            }
            self.top.push(ScoredVectorCandidate {
                distance,
                id: id.to_owned(),
            });
        } else if self.top.peek().is_some_and(|worst| {
            distance
                .total_cmp(&worst.distance)
                .then_with(|| id.cmp(&worst.id))
                .is_lt()
        }) {
            let previous_bytes = self.top.peek().map_or(0, |worst| {
                size_of::<ScoredVectorCandidate>().saturating_add(worst.id.len())
            });
            let previous = self.memory.bytes();
            // Reserve the new owned identity while the previous candidate is still live.
            self.memory.try_grow(id.len())?;
            let replacement = ScoredVectorCandidate {
                distance,
                id: id.to_owned(),
            };
            debug_assert!(self
                .top
                .peek()
                .is_some_and(|worst| replacement.is_better_than(worst)));
            self.top.pop();
            self.top.push(replacement);
            self.memory
                .shrink_to(previous - previous_bytes + candidate_bytes);
        }
        Ok(())
    }

    pub(super) fn ranked(self) -> (Vec<ScoredVectorCandidate>, QueryMemoryReservation) {
        let mut ranked = self.top.into_vec();
        ranked.sort_unstable_by(compare_scored_vector_candidates);
        (ranked, self.memory)
    }
}

pub(super) fn check_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}

pub(super) fn check_result_rows(
    rows: usize,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    if rows > controls.max_result_rows {
        return Err(CassieError::ResourceLimit(format!(
            "query result row limit exceeded: {rows} > {}",
            controls.max_result_rows
        )));
    }
    Ok(())
}

pub(super) fn checked_add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(size_overflow)
}

pub(super) fn checked_mul(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_mul(right).ok_or_else(size_overflow)
}

fn size_overflow() -> CassieError {
    CassieError::ResourceLimit("REST vector retained memory size overflow".to_owned())
}

pub(super) fn json_bytes(value: &serde_json::Value) -> Result<usize, CassieError> {
    crate::runtime::accounted::json::retained_bytes(value)
}

pub(super) fn result_bytes(result: &QueryResult) -> Result<usize, CassieError> {
    let columns = result.columns.iter().try_fold(0usize, |bytes, column| {
        checked_add(
            bytes,
            checked_add(
                2 * size_of::<ColumnMeta>(),
                checked_add(column.name.len(), column.data_type.len())?,
            )?,
        )
    })?;
    result.rows.iter().try_fold(
        checked_add(
            columns,
            checked_add(2 * size_of::<QueryResult>(), result.command.len())?,
        )?,
        |bytes, row| {
            row.iter().try_fold(
                checked_add(
                    bytes,
                    checked_add(
                        2 * size_of::<Vec<Value>>(),
                        checked_mul(
                            row.len(),
                            size_of::<Value>() + size_of::<serde_json::Value>(),
                        )?,
                    )?,
                )?,
                |bytes, value| {
                    let variable = match value {
                        Value::String(text) => text.len(),
                        Value::Vector(vector) => checked_mul(
                            vector.values.len(),
                            size_of::<f32>() + size_of::<serde_json::Value>(),
                        )?,
                        Value::Json(value) => json_bytes(value)?,
                        _ => 0,
                    };
                    checked_add(bytes, variable)
                },
            )
        },
    )
}

pub(super) fn type_name_bytes(data_type: &crate::types::DataType) -> Result<usize, CassieError> {
    use crate::types::DataType;
    Ok(match data_type {
        DataType::Null
        | DataType::Text
        | DataType::Uuid
        | DataType::Date
        | DataType::Time
        | DataType::Json
        | DataType::Char { length: None } => 4,
        DataType::SmallInt => 8,
        DataType::Int => 3,
        DataType::BigInt => 6,
        DataType::Float | DataType::Bytea => 5,
        DataType::Boolean | DataType::Varchar { length: None } => 7,
        DataType::Timestamp => 9,
        DataType::Char {
            length: Some(length),
        } => 6 + decimal_digits(u64::from(*length)),
        DataType::Varchar {
            length: Some(length),
        } => 9 + decimal_digits(u64::from(*length)),
        DataType::Vector(dimensions) => checked_add(
            8,
            decimal_digits(u64::try_from(*dimensions).map_err(|_| size_overflow())?),
        )?,
        DataType::Array(inner) => checked_add(type_name_bytes(inner)?, 2)?,
    })
}

fn decimal_digits(mut number: u64) -> usize {
    let mut digits = 1;
    while number >= 10 {
        number /= 10;
        digits += 1;
    }
    digits
}
