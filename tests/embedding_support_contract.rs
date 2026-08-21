use std::fs;

#[test]
fn should_publish_provider_specific_embedding_support_contracts() {
    // Arrange
    let feature_support =
        fs::read_to_string("docs/feature-support.md").expect("read feature support documentation");
    let readiness = fs::read_to_string("docs/production-readiness.md")
        .expect("read production readiness documentation");
    let evidence = fs::read_to_string("docs/promotion-evidence-matrix.md")
        .expect("read promotion evidence matrix");
    let environment = fs::read_to_string("docs/environment-variables.md")
        .expect("read environment-variable documentation");

    // Act
    let remote_providers = [
        "OpenAI",
        "OpenAI-compatible",
        "TEI",
        "Ollama",
        "Voyage",
        "Cohere",
    ];

    // Assert
    for provider in remote_providers {
        assert!(
            feature_support.contains(&format!("| {provider} embeddings |")),
            "missing provider-specific support row for {provider}"
        );
    }
    assert!(feature_support.contains("| Local deterministic embeddings |"));
    assert!(readiness.contains("HTTP 429"));
    assert!(readiness.contains("mock-provider evidence"));
    assert!(readiness.contains("does not establish hosted availability"));
    assert!(evidence.contains("Provider-specific status contradiction resolved"));
    assert!(evidence.contains("Mock-provider auth and HTTP 429 evidence retained"));
    assert!(environment.contains("CASSIE_OPENAI_MAX_RETRIES"));
    assert!(environment.contains("CASSIE_EMBEDDINGS_MAX_RETRIES"));
    assert!(environment.contains("CASSIE_TEI_MAX_RETRIES"));
    assert!(environment.contains("CASSIE_OLLAMA_MAX_RETRIES"));
    assert!(environment.contains("CASSIE_VOYAGE_MAX_RETRIES"));
    assert!(environment.contains("CASSIE_COHERE_MAX_RETRIES"));
    assert!(environment.contains("does not infer a hosted"));
}
