use super::candidate::{push_top_k, ScoredSearchCandidate};
use super::{
    AnalyzerConfig, BatchRow, BinaryHeap, HybridTopKSpec, QueryError, QueryExecutionControls,
    TokenizedFulltextDocument, TokenizedHybridDocument, Value,
};

type Reservation = crate::runtime::QueryMemoryReservation;

pub(super) struct AccountedScoredCandidates {
    candidates: BinaryHeap<ScoredSearchCandidate>,
    memory: Reservation,
    heap_bytes: usize,
}

impl AccountedScoredCandidates {
    pub(super) fn try_new(
        controls: &QueryExecutionControls,
        top_needed: usize,
    ) -> Result<Self, QueryError> {
        let capacity = top_needed.saturating_add(1);
        let heap_bytes = capacity.saturating_mul(std::mem::size_of::<ScoredSearchCandidate>());
        let memory = controls.reserve_query_memory(heap_bytes)?;
        Ok(Self {
            candidates: BinaryHeap::with_capacity(capacity),
            memory,
            heap_bytes,
        })
    }

    pub(super) fn try_push(
        &mut self,
        top_needed: usize,
        sort_value: f64,
        score: f64,
        id: &str,
    ) -> Result<(), QueryError> {
        self.memory.try_grow(id.len())?;
        push_top_k(
            &mut self.candidates,
            top_needed,
            ScoredSearchCandidate {
                sort_value,
                score,
                id: id.to_string(),
            },
        );
        self.retain_current_bytes();
        Ok(())
    }

    pub(super) fn try_push_existing(
        &mut self,
        top_needed: usize,
        candidate: ScoredSearchCandidate,
    ) -> Result<(), QueryError> {
        self.memory.try_grow(candidate.id.len())?;
        push_top_k(&mut self.candidates, top_needed, candidate);
        self.retain_current_bytes();
        Ok(())
    }

    pub(super) fn into_parts(self) -> (BinaryHeap<ScoredSearchCandidate>, Reservation) {
        (self.candidates, self.memory)
    }

    fn retain_current_bytes(&mut self) {
        let retained_bytes = self.heap_bytes.saturating_add(
            self.candidates
                .iter()
                .map(|candidate| candidate.id.len())
                .sum::<usize>(),
        );
        self.memory.shrink_to(retained_bytes);
    }
}

pub(super) fn batch_row_bytes(row: &BatchRow) -> usize {
    row.entries()
        .iter()
        .map(|(name, value)| name.len().saturating_add(format!("{value:?}").len()))
        .sum()
}

pub(super) fn reserve_hybrid_documents(
    controls: &QueryExecutionControls,
    rows: &[BatchRow],
    spec: &HybridTopKSpec,
    analyzer: &AnalyzerConfig,
) -> Result<Reservation, QueryError> {
    let bytes = rows.iter().fold(0usize, |total, row| {
        let id_bytes = row.get("id").and_then(Value::as_str).map_or(0, str::len);
        let text_bytes = row
            .get(&spec.text_field)
            .and_then(Value::as_str)
            .map_or(0, |text| tokenized_text_upper_bound(text, analyzer));
        let vector_bytes = row.get(&spec.vector_field).map_or(0, |value| match value {
            Value::Vector(vector) => vector
                .values
                .len()
                .saturating_mul(std::mem::size_of::<f32>()),
            Value::Json(value) => value.as_array().map_or(0, |values| {
                values.len().saturating_mul(std::mem::size_of::<f32>())
            }),
            _ => 0,
        });
        total
            .saturating_add(std::mem::size_of::<TokenizedHybridDocument>())
            .saturating_add(id_bytes)
            .saturating_add(text_bytes)
            .saturating_add(vector_bytes)
    });
    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

pub(super) fn reserve_tokenized_fulltext_documents(
    controls: &QueryExecutionControls,
    documents: &[crate::midge::adapter::DocumentRef],
    text_field: &str,
    analyzer: &AnalyzerConfig,
) -> Result<Reservation, QueryError> {
    let bytes = documents.iter().fold(0usize, |total, document| {
        let text_bytes = document
            .payload
            .get(text_field)
            .and_then(serde_json::Value::as_str)
            .map_or(0, |text| tokenized_text_upper_bound(text, analyzer));
        total
            .saturating_add(std::mem::size_of::<TokenizedFulltextDocument>())
            .saturating_add(document.id.len())
            .saturating_add(text_bytes)
    });
    controls
        .reserve_query_memory(bytes)
        .map_err(QueryError::from)
}

pub(super) fn tokenized_text_upper_bound(text: &str, analyzer: &AnalyzerConfig) -> usize {
    let token_count = if analyzer.tokenizer == "whitespace" {
        text.split_whitespace().count()
    } else {
        text.split(|character: char| !character.is_alphanumeric())
            .filter(|token| !token.is_empty())
            .count()
    };
    let normalized_bytes = text.len().saturating_mul(3);
    let per_token = std::mem::size_of::<String>()
        .saturating_add(2 * std::mem::size_of::<(String, usize)>())
        .saturating_add(2 * std::mem::size_of::<usize>());
    normalized_bytes
        .saturating_mul(4)
        .saturating_add(token_count.saturating_mul(per_token))
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use crate::config::CassieRuntimeLimits;

    use super::{AccountedScoredCandidates, QueryExecutionControls, ScoredSearchCandidate};

    #[test]
    fn should_reserve_before_retaining_a_scored_candidate_given_an_exhausted_budget() {
        // Arrange
        let heap_bytes = 2 * std::mem::size_of::<ScoredSearchCandidate>();
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: heap_bytes + 3,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let mut top =
            AccountedScoredCandidates::try_new(&controls, 1).expect("accounted top-k heap");
        top.try_push(1, -1.0, 1.0, "one")
            .expect("first retained candidate");

        // Act
        let error = top
            .try_push(1, -2.0, 2.0, "candidate-too-large")
            .expect_err("candidate must be rejected before cloning");

        // Assert
        assert!(error.to_string().contains("memory"));
        assert_eq!(controls.current_query_memory_bytes(), heap_bytes + 3);
        let (candidates, memory) = top.into_parts();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates.peek().map(|candidate| candidate.id.as_str()),
            Some("one")
        );
        drop(memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
