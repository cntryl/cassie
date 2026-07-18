use super::{BatchRow, BinaryHeap, CmpOrdering, QueryError, QueryExecutionControls, Value};

type Reservation = crate::runtime::QueryMemoryReservation;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ScoredSearchCandidate {
    pub(super) sort_value: f64,
    pub(super) score: f64,
    pub(super) id: String,
}

impl ScoredSearchCandidate {
    fn is_better_than(&self, other: &Self) -> bool {
        compare_scored_search_candidates(self, other) == CmpOrdering::Less
    }
}

impl Eq for ScoredSearchCandidate {}

impl PartialOrd for ScoredSearchCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<CmpOrdering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredSearchCandidate {
    fn cmp(&self, other: &Self) -> CmpOrdering {
        compare_scored_search_candidates(self, other)
    }
}

fn compare_scored_search_candidates(
    left: &ScoredSearchCandidate,
    right: &ScoredSearchCandidate,
) -> CmpOrdering {
    left.sort_value
        .total_cmp(&right.sort_value)
        .then_with(|| left.id.cmp(&right.id))
}

pub(super) fn push_top_k(
    top: &mut BinaryHeap<ScoredSearchCandidate>,
    top_needed: usize,
    candidate: ScoredSearchCandidate,
) {
    if top.len() < top_needed {
        top.push(candidate);
    } else if top
        .peek()
        .is_some_and(|worst| candidate.is_better_than(worst))
    {
        top.pop();
        top.push(candidate);
    }
}

pub(super) fn scored_candidates_to_rows(
    top: BinaryHeap<ScoredSearchCandidate>,
    offset: usize,
    limit: usize,
    id_column: &str,
    score_column: &str,
    controls: &QueryExecutionControls,
) -> Result<AccountedScoredRows, QueryError> {
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_scored_search_candidates);
    let selected = ranked.iter().skip(offset).take(limit);
    let output_bytes = selected.fold(0usize, |total, candidate| {
        total.saturating_add(scored_row_retained_bytes(
            candidate,
            id_column,
            score_column,
        ))
    });
    let memory = controls.reserve_query_memory(output_bytes)?;
    let mut rows = Vec::with_capacity(limit.min(ranked.len().saturating_sub(offset)));
    for candidate in ranked.into_iter().skip(offset).take(limit) {
        rows.push(BatchRow::from_projected_values(vec![
            (id_column.to_string(), Value::String(candidate.id)),
            (score_column.to_string(), Value::Float64(candidate.score)),
        ]));
    }
    Ok(AccountedScoredRows { rows, memory })
}

pub(super) struct AccountedScoredRows {
    rows: Vec<BatchRow>,
    memory: Reservation,
}

impl AccountedScoredRows {
    pub(super) fn into_parts(self) -> (Vec<BatchRow>, Reservation) {
        (self.rows, self.memory)
    }
}

fn scored_row_retained_bytes(
    candidate: &ScoredSearchCandidate,
    id_column: &str,
    score_column: &str,
) -> usize {
    let entry_bytes = 2usize.saturating_mul(std::mem::size_of::<(String, Value)>());
    std::mem::size_of::<BatchRow>()
        .saturating_add(entry_bytes)
        .saturating_add(candidate.id.len())
        .saturating_add(id_column.len().saturating_add(score_column.len()))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crate::config::CassieRuntimeLimits;

    use super::{
        scored_candidates_to_rows, BinaryHeap, QueryExecutionControls, ScoredSearchCandidate,
    };

    #[test]
    fn should_reject_scored_rows_before_output_construction_given_a_late_stage_budget() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 64,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let earlier_stage = controls
            .reserve_query_memory(64)
            .expect("earlier-stage reservation");
        let top = BinaryHeap::from([ScoredSearchCandidate {
            sort_value: -1.0,
            score: 1.0,
            id: "row-0001".to_string(),
        }]);

        // Act
        let Err(error) = scored_candidates_to_rows(top, 0, 1, "id", "score", &controls) else {
            panic!("final rows must reserve before allocation");
        };

        // Assert
        assert!(error.to_string().contains("memory"));
        assert_eq!(controls.current_query_memory_bytes(), 64);
        drop(earlier_stage);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
