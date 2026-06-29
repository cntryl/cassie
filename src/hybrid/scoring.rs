#[derive(Debug, Clone)]
pub struct HybridScorePolicy {
    pub search_weight: f64,
    pub vector_weight: f64,
}

impl Default for HybridScorePolicy {
    fn default() -> Self {
        Self {
            search_weight: 0.65,
            vector_weight: 0.35,
        }
    }
}

#[must_use]
pub fn hybrid_score(
    search_score: f64,
    vector_score: f64,
    policy: Option<&HybridScorePolicy>,
) -> f64 {
    let fallback = HybridScorePolicy::default();
    let policy = policy.unwrap_or(&fallback);
    let score = search_score * policy.search_weight + vector_score * policy.vector_weight;
    (score * 1_000_000_000_000.0).round() / 1_000_000_000_000.0
}
