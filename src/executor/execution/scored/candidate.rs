use super::{BatchRow, BinaryHeap, CmpOrdering, Value};

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
) -> Vec<BatchRow> {
    let mut ranked = top.into_vec();
    ranked.sort_by(compare_scored_search_candidates);
    ranked
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|candidate| {
            BatchRow::new(vec![
                (id_column.to_string(), Value::String(candidate.id)),
                (score_column.to_string(), Value::Float64(candidate.score)),
            ])
        })
        .collect()
}
