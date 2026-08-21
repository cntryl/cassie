#[test]
fn should_wire_a_pinned_sqlx_fixture_into_the_opt_in_workflow() {
    // Arrange
    let workflow = include_str!("../.github/workflows/compatibility-probes.yml");
    let fixture = include_str!("fixtures/sqlx_probe/Cargo.toml");
    let documentation = include_str!("../docs/compatibility-probe-contract.md");

    // Act
    let enables_probe = workflow.contains("CASSIE_RUN_SQLX_COMPAT");
    let invokes_fixture = workflow.contains("--test compatibility_sqlx");
    let records_client_version = workflow.contains("client_version=${SQLX_VERSION}");
    let pins_sqlx = fixture.contains("sqlx = { version = \"=0.8.3\"");
    let documents_probe = documentation.contains("Dedicated opt-in fixture");

    // Assert
    assert!(enables_probe);
    assert!(invokes_fixture);
    assert!(records_client_version);
    assert!(pins_sqlx);
    assert!(documents_probe);
}
