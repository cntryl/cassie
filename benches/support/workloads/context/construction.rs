use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;

use super::{
    benchmark_data_dir_for_mode, configure_benchmark_environment, prepare_collection, BenchContext,
    BenchIndexOptions, BenchmarkStorageMode,
};

pub(super) fn context_with_index_options(
    label: &str,
    dataset_rows: usize,
    index_options: BenchIndexOptions,
) -> Result<BenchContext, CassieError> {
    context_with_index_options_and_runtime(
        label,
        dataset_rows,
        index_options,
        BenchmarkStorageMode::Default,
        |_| {},
    )
}

pub(super) fn context_with_index_options_and_runtime(
    label: &str,
    dataset_rows: usize,
    index_options: BenchIndexOptions,
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
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        data_dir: dir,
        _embedding_server: None,
    };
    prepare_collection(&ctx, dataset_rows, index_options)?;
    Ok(ctx)
}
