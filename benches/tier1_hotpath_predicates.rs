#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier1_hotpath_predicates");
    runner.tier1_micro(
        "field_lookup_by_field_id",
        workloads::field_lookup_by_field_id,
    );
    runner.tier1_micro("predicate_evaluation", workloads::predicate_evaluation);
    runner.tier1_micro("query_parameter_binding", workloads::parameter_binding);
    runner.finish();
}
