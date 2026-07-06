#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_predicates");
    runner.micro(
        stress::StressCase::tier1_micro("field_lookup_by_field_id")
            .metadata("logical_operations_per_iteration", "64"),
        workloads::field_lookup_by_field_id,
    );
    runner.tier1_micro("predicate_evaluation", workloads::predicate_evaluation);
    runner.tier1_micro("query_parameter_binding", workloads::parameter_binding);
    runner.finish();
}
