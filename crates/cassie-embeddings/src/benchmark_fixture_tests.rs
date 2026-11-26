use crate::local::{LocalProvider, LocalProviderConfig};
use crate::EmbeddingProvider;

const DIMENSIONS: usize = 768;

fn provider() -> LocalProvider {
    LocalProvider::with_config(LocalProviderConfig {
        model: "cassie-benchmark-local-v1".to_owned(),
        dimensions: DIMENSIONS,
    })
    .expect("local benchmark provider config")
}

#[test]
fn should_return_a_deterministic_local_query_embedding_at_768_dimensions() {
    // Arrange
    let provider = provider();
    let input = "event sourced read models";

    // Act
    let first = provider.embed_query(input).expect("query embedding");
    let second = provider.embed_query(input).expect("repeat query embedding");

    // Assert
    assert_eq!(first.values.len(), DIMENSIONS);
    assert_eq!(first.values, second.values);
}

#[test]
fn should_embed_32_documents_at_768_dimensions() {
    // Arrange
    let provider = provider();
    let inputs = (0..32)
        .map(|index| format!("projection event {index}"))
        .collect::<Vec<_>>();

    // Act
    let embeddings = provider
        .embed_documents(&inputs)
        .expect("document embeddings");

    // Assert
    assert_eq!(embeddings.len(), 32);
    assert!(embeddings
        .iter()
        .all(|embedding| embedding.values.len() == DIMENSIONS));
}
