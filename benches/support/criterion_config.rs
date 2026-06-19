//! Criterion configuration helpers for Cassie's tiered benchmark suite.

use criterion::Criterion;
use std::time::Duration;

fn env_duration_ms(name: &str, default_ms: u64) -> Duration {
    let millis = std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default_ms);

    Duration::from_millis(millis)
}

fn env_usize(name: &str, default_value: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default_value)
}

fn env_f64(name: &str, default_value: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(default_value)
}

#[allow(dead_code)]
pub fn criterion_config_for_tier1() -> Criterion {
    Criterion::default()
        .warm_up_time(env_duration_ms("BENCH_TIER1_WARMUP_MS", 100))
        .measurement_time(env_duration_ms("BENCH_TIER1_MEASUREMENT_MS", 500))
        .sample_size(env_usize("BENCH_TIER1_SAMPLE_SIZE", 12))
        .noise_threshold(env_f64("BENCH_TIER1_NOISE_THRESHOLD", 0.05))
        .without_plots()
}

#[allow(dead_code)]
pub fn criterion_config_for_tier2() -> Criterion {
    Criterion::default()
        .warm_up_time(env_duration_ms("BENCH_TIER2_WARMUP_MS", 150))
        .measurement_time(env_duration_ms("BENCH_TIER2_MEASUREMENT_MS", 700))
        .sample_size(env_usize("BENCH_TIER2_SAMPLE_SIZE", 10))
        .noise_threshold(env_f64("BENCH_TIER2_NOISE_THRESHOLD", 0.05))
        .without_plots()
}

#[allow(dead_code)]
pub fn criterion_config_for_tier2_write() -> Criterion {
    Criterion::default()
        .warm_up_time(env_duration_ms("BENCH_TIER2_WRITE_WARMUP_MS", 250))
        .measurement_time(env_duration_ms("BENCH_TIER2_WRITE_MEASUREMENT_MS", 20_000))
        .sample_size(env_usize("BENCH_TIER2_WRITE_SAMPLE_SIZE", 10))
        .noise_threshold(env_f64("BENCH_TIER2_WRITE_NOISE_THRESHOLD", 0.05))
        .without_plots()
}

#[allow(dead_code)]
pub fn criterion_config_for_tier3() -> Criterion {
    Criterion::default()
        .warm_up_time(env_duration_ms("BENCH_TIER3_WARMUP_MS", 200))
        .measurement_time(env_duration_ms("BENCH_TIER3_MEASUREMENT_MS", 2_500))
        .sample_size(env_usize("BENCH_TIER3_SAMPLE_SIZE", 10))
        .noise_threshold(env_f64("BENCH_TIER3_NOISE_THRESHOLD", 0.05))
        .without_plots()
}

#[allow(dead_code)]
pub fn criterion_config_for_tier4() -> Criterion {
    Criterion::default()
        .warm_up_time(env_duration_ms("BENCH_TIER4_WARMUP_MS", 250))
        .measurement_time(env_duration_ms("BENCH_TIER4_MEASUREMENT_MS", 1_500))
        .sample_size(env_usize("BENCH_TIER4_SAMPLE_SIZE", 10))
        .noise_threshold(env_f64("BENCH_TIER4_NOISE_THRESHOLD", 0.05))
        .without_plots()
}

#[allow(dead_code)]
pub fn criterion_config_for_tier4_http() -> Criterion {
    Criterion::default()
        .warm_up_time(env_duration_ms("BENCH_TIER4_HTTP_WARMUP_MS", 250))
        .measurement_time(env_duration_ms("BENCH_TIER4_HTTP_MEASUREMENT_MS", 6_500))
        .sample_size(env_usize("BENCH_TIER4_HTTP_SAMPLE_SIZE", 10))
        .noise_threshold(env_f64("BENCH_TIER4_HTTP_NOISE_THRESHOLD", 0.05))
        .without_plots()
}
