#![allow(dead_code, unused_imports)]

use std::future::{ready, Ready};
use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;

use super::context::{
    benchmark_data_dir_for_mode, configure_benchmark_environment, BenchContext,
    BenchmarkStorageMode, ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
};

pub fn empty_context(label: &str) -> Ready<Result<BenchContext, CassieError>> {
    ready(empty_context_with_config(
        label,
        BenchmarkStorageMode::Default,
        |_| {},
    ))
}

pub fn empty_context_with_temp_budget(
    label: &str,
    dataset_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(empty_context_with_config(
        label,
        BenchmarkStorageMode::Default,
        |config| {
            config.limits.max_result_rows = dataset_rows;
            config.limits.query_memory_budget_bytes = ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
        },
    ))
}

pub fn empty_disk_context_with_temp_budget(
    label: &str,
    dataset_rows: usize,
    query_memory_budget_bytes: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(empty_context_with_config(
        label,
        BenchmarkStorageMode::Disk,
        |config| {
            config.limits.max_result_rows = dataset_rows;
            config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
            config.limits.query_timeout_ms = 0;
        },
    ))
}

fn empty_context_with_config(
    label: &str,
    storage_mode: BenchmarkStorageMode,
    configure: impl FnOnce(&mut CassieRuntimeConfig),
) -> Result<BenchContext, CassieError> {
    configure_benchmark_environment();
    if matches!(storage_mode, BenchmarkStorageMode::Disk) {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }
    let dir = benchmark_data_dir_for_mode(label, storage_mode);
    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    configure(&mut config);
    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(dir.clone(), config)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    Ok(BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        data_dir: dir,
        _embedding_server: None,
    })
}
