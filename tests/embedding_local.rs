use cassie::embeddings::local::{LocalProvider, LocalProviderConfig};
use cassie::embeddings::EmbeddingProvider;

fn magnitude(values: &[f32]) -> f32 {
    values.iter().map(|value| value * value).sum::<f32>().sqrt()
}

#[test]
fn should_produce_deterministic_local_document_embeddings() {
    // Arrange
    let provider = LocalProvider::with_config(LocalProviderConfig {
        model: "cassie-local-hash-v1".to_string(),
        dimensions: 8,
    })
    .expect("provider should configure");
    let inputs = vec!["alpha".to_string(), "beta".to_string()];

    // Act
    let first = provider
        .embed_documents(&inputs)
        .expect("first embedding pass");
    let second = provider
        .embed_documents(&inputs)
        .expect("second embedding pass");

    // Assert
    assert_eq!(provider.provider_name(), "local");
    assert_eq!(provider.model_name(), "cassie-local-hash-v1");
    assert_eq!(provider.dimensions(), 8);
    assert_eq!(first.len(), second.len());
    for (lhs, rhs) in first.iter().zip(&second) {
        assert_eq!(lhs.values, rhs.values);
    }
    assert_eq!(first[0].values.len(), 8);
}

#[test]
fn should_normalize_local_embeddings_to_unit_length() {
    // Arrange
    let provider = LocalProvider::with_config(LocalProviderConfig {
        model: "cassie-local-hash-v1".to_string(),
        dimensions: 16,
    })
    .expect("provider should configure");

    // Act
    let document = provider
        .embed_documents(&["alpha".to_string()])
        .expect("document embedding")
        .remove(0);
    let query = provider.embed_query("alpha").expect("query embedding");

    // Assert
    assert!((magnitude(&document.values) - 1.0).abs() < 1e-5);
    assert!((magnitude(&query.values) - 1.0).abs() < 1e-5);
}

#[test]
fn should_distinguish_local_query_embeddings_from_document_embeddings() {
    // Arrange
    let provider = LocalProvider::with_config(LocalProviderConfig {
        model: "cassie-local-hash-v1".to_string(),
        dimensions: 12,
    })
    .expect("provider should configure");

    // Act
    let document = provider
        .embed_documents(&["alpha".to_string()])
        .expect("document embedding")
        .remove(0);
    let query = provider.embed_query("alpha").expect("query embedding");

    // Assert
    assert_ne!(document.values, query.values);
    assert_eq!(document.values.len(), 12);
    assert_eq!(query.values.len(), 12);
}
