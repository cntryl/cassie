#[test]
fn should_cleanup_transport_soak_fixture_after_measurement() {
    // Arrange
    let source = include_str!("../benches/tier6_soak_transport.rs");

    // Act
    let shuts_down_cassie = source.contains("context.cassie.shutdown()");
    let removes_data_dir = source.contains("std::fs::remove_dir_all(&data_dir)");
    let removes_marker_file = source.contains("std::fs::remove_file(&data_dir)");
    let verifies_cleanup = source.contains("assert!(!data_dir.exists()");

    // Assert
    assert!(shuts_down_cassie);
    assert!(removes_data_dir);
    assert!(removes_marker_file);
    assert!(verifies_cleanup);
}

#[test]
fn should_own_only_generated_tls_material_given_http_benchmark_configuration() {
    // Arrange
    let tls_source = include_str!("../benches/support/workloads/http.rs");
    let callers = [
        include_str!("../benches/tier4_integration_http.rs"),
        include_str!("../benches/tier4_integration_protocol_compare.rs"),
        include_str!("../benches/tier5_scaling_transport.rs"),
        include_str!("../benches/tier6_soak_transport.rs"),
    ];

    // Act
    let returns_generated_ownership =
        tls_source.contains("Result<Option<GeneratedHttpTlsMaterial>, CassieError>");
    let leaves_user_paths_unowned = tls_source.contains("return Ok(None);");
    let generated_material_has_cleanup = tls_source.contains("impl GeneratedHttpTlsMaterial")
        && tls_source.contains("pub fn cleanup(");
    let every_caller_retains_and_cleans = callers
        .iter()
        .all(|source| source.contains("generated_http_tls") && source.contains(".cleanup()"));

    // Assert
    assert!(returns_generated_ownership);
    assert!(leaves_user_paths_unowned);
    assert!(generated_material_has_cleanup);
    assert!(every_caller_retains_and_cleans);
}

#[test]
fn should_enforce_configured_tier6_result_row_bounds() {
    // Arrange
    let mixed = include_str!("../benches/tier6_soak_mixed.rs");
    let transport = include_str!("../benches/tier6_soak_transport.rs");

    // Act
    let mixed_configures_bound = mixed.contains("TIER6_MAX_RESULT_ROWS")
        && mixed.contains("context_with_mock_tei_embeddings(")
        && mixed.contains("\"configured_max_result_rows\"");
    let transport_configures_bound = transport.contains("TIER6_MAX_RESULT_ROWS")
        && transport.contains("scalar_context(")
        && transport.contains("\"configured_max_result_rows\"");
    let every_mixed_result_is_gated = mixed.contains("assert_result_cardinality_within_bound(");
    let every_transport_result_is_gated = transport
        .matches("assert_result_cardinality_within_bound(")
        .count()
        >= 4;

    // Assert
    assert!(mixed_configures_bound);
    assert!(transport_configures_bound);
    assert!(every_mixed_result_is_gated);
    assert!(every_transport_result_is_gated);
}
