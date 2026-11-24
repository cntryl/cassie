#[path = "support/fixture_dir.rs"]
mod fixture_dir;
#[path = "support/ordered_dashboard.rs"]
mod ordered_dashboard;
#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier5,
        "tier5_ordered_dashboard",
    );
    ordered_dashboard::run(&mut runner);
    runner.finish();
}
