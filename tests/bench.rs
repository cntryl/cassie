use std::fs;
use std::path::PathBuf;

use cassie::bench::{self, BenchmarkConfig, BenchmarkRecord};
use uuid::Uuid;

fn data_dir(label: &str) -> String {
    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-bench-{}-{}", label, Uuid::new_v4()));
    dir.to_string_lossy().to_string()
}

#[test]
fn should_parse_benchmark_config_arguments() {
    // Arrange
    let args = vec![
        "cassie-bench".to_string(),
        "--workload".to_string(),
        "row_encode_decode".to_string(),
        "--dataset".to_string(),
        "10k".to_string(),
        "--iterations".to_string(),
        "12".to_string(),
        "--warmup".to_string(),
        "3".to_string(),
        "--output-dir".to_string(),
        "/tmp/cassie-bench-output".to_string(),
    ];

    // Act
    let config = BenchmarkConfig::parse_args(args).expect("config should parse");

    // Assert
    assert_eq!(config.workload, "row_encode_decode");
    assert_eq!(config.dataset, "10k");
    assert_eq!(config.iterations, 12);
    assert_eq!(config.warmup, 3);
    assert_eq!(config.output_dir, PathBuf::from("/tmp/cassie-bench-output"));
}

#[test]
fn should_serialize_benchmark_record_fields() {
    // Arrange
    let record = BenchmarkRecord {
        tier: 1,
        name: "row_encode_decode".to_string(),
        dataset: "tiny".to_string(),
        rows: 1,
        duration_ms: 1,
        p50_ms: 1,
        p95_ms: 1,
        p99_ms: 1,
        throughput: 1.0,
        allocations: 0,
        bytes_allocated: 0,
        cpu_percent: 0.0,
        memory_mb: 0.0,
    };

    // Act
    let json = serde_json::to_value(&record).expect("serialize should work");

    // Assert
    assert_eq!(json["tier"], 1);
    assert_eq!(json["name"], "row_encode_decode");
    assert!(json.get("throughput").is_some());
    assert!(json.get("cpu_percent").is_some());
    assert!(json.get("memory_mb").is_some());
}

#[test]
fn should_run_minimal_benchmark_write_output_json() {
    // Arrange
    let output_dir = data_dir("smoke");
    let config = BenchmarkConfig {
        workload: "row_encode_decode".to_string(),
        dataset: "tiny".to_string(),
        iterations: 1,
        warmup: 0,
        output_dir: PathBuf::from(&output_dir),
        mode: Default::default(),
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let record = bench::run(&config).await.expect("benchmark should run");

        // Assert
        assert_eq!(record.name, "row_encode_decode");
        let output_file = PathBuf::from(&output_dir).join("row_encode_decode.json");
        assert!(output_file.exists());
        let raw = fs::read_to_string(output_file).expect("benchmark output should exist");
        let json: serde_json::Value = serde_json::from_str(&raw).expect("json should parse");
        assert_eq!(json["name"], "row_encode_decode");
    });

    let _ = fs::remove_dir_all(output_dir);
}
