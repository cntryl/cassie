use cassie::bench::{self, BenchmarkConfig};

#[tokio::main]
async fn main() -> Result<(), cassie::CassieError> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let config = BenchmarkConfig::parse_args(std::env::args())?;
    if config.workload == "__list__" {
        for workload in bench::available_workloads() {
            println!("{workload}");
        }
        return Ok(());
    }

    let record = bench::run(&config).await?;
    println!(
        "{}",
        serde_json::to_string_pretty(&record).unwrap_or_else(|_| "{}".to_string())
    );
    Ok(())
}
