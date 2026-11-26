#[path = "support/fixture_dir.rs"]
mod fixture_dir;
#[path = "support/workloads/time_series_mixed.rs"]
mod workload;

fn main() {
    workload::run();
}
