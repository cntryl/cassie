#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_search_vector");
    runner.tier1_micro("tokenization", workloads::tokenization);
    runner.micro(
        stress::StressCase::tier1_micro("bm25_score")
            .metadata("logical_operations_per_iteration", "8"),
        workloads::bm25_score,
    );
    runner.micro(
        stress::StressCase::tier1_micro("cosine_distance")
            .metadata("logical_operations_per_iteration", "32"),
        workloads::cosine_distance,
    );
    runner.micro(
        stress::StressCase::tier1_micro("dot_product")
            .metadata("logical_operations_per_iteration", "32"),
        workloads::dot_product,
    );
    runner.micro(
        stress::StressCase::tier1_micro("l2_distance")
            .metadata("logical_operations_per_iteration", "32"),
        workloads::l2_distance,
    );
    runner.tier1_micro("hnsw_candidate_search", workloads::hnsw_candidate_search);
    runner.finish();
}
