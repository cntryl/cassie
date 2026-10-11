use super::*;
use cassie::app::CassieSession;
use cassie::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Default)]
struct ConstantProvider(AtomicUsize);

impl EmbeddingProvider for ConstantProvider {
    fn provider_name(&self) -> &'static str { "local" }
    fn model_name(&self) -> &'static str { "deterministic-test" }
    fn dimensions(&self) -> usize { 3 }
    fn embed_documents(&self, inputs: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(inputs.iter().map(|_| Embedding { values: vec![1.0, 0.0, 0.0] }).collect())
    }
}

fn cache_instance(path: &str) -> (Cassie, Arc<ConstantProvider>) {
    let config = CassieRuntimeConfig {
        embeddings: EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "deterministic-test".into(), dimensions: 3,
        }),
        ..CassieRuntimeConfig::default()
    };
    let mut cassie = Cassie::new_with_data_dir_and_config(path, config).expect("Cassie");
    let provider = Arc::new(ConstantProvider::default());
    cassie.embedding_provider = provider.clone();
    cassie.startup().expect("startup");
    (cassie, provider)
}

fn metric(cassie: &Cassie, field: &str) -> u64 {
    cassie.metrics()["execution_result_cache"][field].as_u64().expect("present cache counter")
}

fn assert_rows(cassie: &Cassie, session: &CassieSession, embedded: bool) {
    let value = if embedded { vector([1.0, 0.0, 0.0]) } else { Value::Null };
    assert_eq!(cassie.execute_sql(session,
        "SELECT id, embedding FROM cache_backfill_rows ORDER BY id", vec![])
        .expect("cached scalar query").rows,
        vec![vec![Value::String("a".into()), value.clone()],
             vec![Value::String("b".into()), value]]);
}

fn warm_reader(cassie: &Cassie, session: &CassieSession, embedded: bool) {
    assert_rows(cassie, session, embedded);
    let hits = metric(cassie, "hits");
    assert_rows(cassie, session, embedded);
    assert_eq!(metric(cassie, "hits"), hits + 1, "prove a hit for this reader identity");
}

#[test]
fn should_invalidate_both_warm_readers_after_a_committed_backfill_error() {
    // Arrange
    let _guard = cassie::midge::adapter::document_write_failure_point_test_guard();
    use_local_storage();
    let path = data_dir("cache_two_readers_backfill_failure_restart");
    let (cassie, provider) = cache_instance(&path);
    create_table(&cassie, "cache_backfill_rows", false);
    run(&cassie, "INSERT INTO cache_backfill_rows (id, body) VALUES ('a', 'hello'), ('b', 'world')", vec![]);
    let first = cassie.create_session("cache_reader_a", None);
    let second = cassie.create_session("cache_reader_b", None);
    warm_reader(&cassie, &first, false);
    warm_reader(&cassie, &second, false);
    assert_eq!(metric(&cassie, "entries"), 2, "two separately warm identities");
    let epoch = cassie.midge.data_epoch().expect("Data epoch");
    cassie::midge::adapter::set_document_write_failure_point(Some(
        cassie::midge::adapter::DocumentWriteFailurePoint::VectorState));

    // Act
    let result = rest_index(&cassie, "cache_backfill_rows");
    cassie::midge::adapter::set_document_write_failure_point(None);

    // Assert
    let error = result.expect_err("actual Data-applied sidecar failure");
    assert!(error.to_string().contains("recoverable vector index publication"));
    assert!(provider.0.load(Ordering::SeqCst) > 0, "backfill actually called provider");
    assert_eq!(cassie.midge.data_epoch().expect("committed Data epoch"), epoch + 1);
    assert_eq!(metric(&cassie, "entries"), 0, "committed failure evicts both stale readers");
    assert_eq!(metric(&cassie, "bytes"), 0);
    let hits = metric(&cassie, "hits");
    let misses = metric(&cassie, "misses");
    assert_rows(&cassie, &first, true);
    assert_rows(&cassie, &second, true);
    assert_eq!(metric(&cassie, "hits"), hits, "neither reader consumes stale results");
    assert_eq!(metric(&cassie, "misses"), misses + 2);
    warm_reader(&cassie, &first, true);
    warm_reader(&cassie, &second, true);
    drop(error);
    drop(first);
    drop(second);
    drop(provider);
    cassie.shutdown();
    drop(cassie);
    assert_cold_cache_recovery(&path, epoch + 1);
    std::fs::remove_dir_all(&path).expect("cleanup after all restart owners drop");
    assert!(!std::path::Path::new(&path).exists());
}

fn assert_cold_cache_recovery(path: &str, expected_epoch: u64) {
    let (cassie, provider) = cache_instance(path);
    crate::vector_publication_support::assert_cleanup(&cassie);
    assert_eq!(cassie.midge.data_epoch().expect("recovered Data epoch"), expected_epoch);
    assert_eq!(metric(&cassie, "entries"), 0, "startup does not restore execution results");
    assert_eq!(metric(&cassie, "bytes"), 0);
    let first = cassie.create_session("cache_reader_a", None);
    let second = cassie.create_session("cache_reader_b", None);
    warm_reader(&cassie, &first, true);
    warm_reader(&cassie, &second, true);
    assert_eq!(provider.0.load(Ordering::SeqCst), 0, "recovery and scalar reads do not re-embed");
    drop(first);
    drop(second);
    drop(provider);
    cassie.shutdown();
    drop(cassie);
}
