use std::future::{ready, Ready};
use std::sync::Arc;

use cassie::app::{Cassie, CassieError};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, SelfHostedEmbeddingRuntimeConfig,
};

use super::context::{
    benchmark_data_dir, configure_benchmark_environment, prepare_collection, BenchContext,
    BenchIndexOptions, ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
};
use super::mock_tei::MockTeiEmbeddingServer;

pub fn context_with_mock_tei_embeddings(
    label: &str,
    dataset_rows: usize,
    max_result_rows: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(context_with_mock_tei_embeddings_now(
        label,
        dataset_rows,
        max_result_rows,
    ))
}

fn context_with_mock_tei_embeddings_now(
    label: &str,
    dataset_rows: usize,
    max_result_rows: usize,
) -> Result<BenchContext, CassieError> {
    configure_benchmark_environment();
    let server = Arc::new(MockTeiEmbeddingServer::spawn());
    let mut config = CassieRuntimeConfig::from_env()
        .map_err(|error| CassieError::Configuration(error.to_string()))?;
    config.embeddings = EmbeddingsRuntimeConfig::Tei(SelfHostedEmbeddingRuntimeConfig {
        base_url: server.base_url(),
        model: "BAAI/bge-small-en-v1.5".to_string(),
        dimensions: 3,
        timeout_seconds: 2,
        max_batch_size: 16,
        max_retries: 1,
    });
    config.limits.query_memory_budget_bytes = ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES;
    config.limits.max_result_rows = max_result_rows;
    let dir = benchmark_data_dir(label);
    let cassie = Arc::new(Cassie::new_with_data_dir_and_config(dir.clone(), config)?);
    cassie.startup()?;
    let session = cassie.create_session("benchmark", None);
    let ctx = BenchContext {
        cassie,
        session,
        collection: "bench_documents".to_string(),
        data_dir: dir,
        _embedding_server: Some(server),
    };
    prepare_collection(&ctx, dataset_rows, BenchIndexOptions::full())?;
    let _ = ctx.cassie.execute_sql(
        &ctx.session,
        "CREATE INDEX bench_documents_embedding_idx ON bench_documents USING vector (embedding) WITH (source_field = body, metric = cosine)",
        vec![],
    )?;
    Ok(ctx)
}
