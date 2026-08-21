#[test]
fn should_wire_a_pinned_diesel_fixture_into_the_opt_in_workflow() {
    // Arrange
    let workflow = include_str!("../.github/workflows/compatibility-probes.yml");
    let fixture = include_str!("fixtures/diesel_probe/Cargo.toml");
    let documentation = include_str!("../docs/compatibility-probe-contract.md");

    // Act
    let enables_probe = workflow.contains("CASSIE_RUN_DIESEL_COMPAT");
    let invokes_fixture = workflow.contains("--test compatibility_diesel");
    let records_client_version = workflow.contains("client_version=${DIESEL_VERSION}");
    let records_failed_probe = workflow.contains("status=failed");
    let pins_diesel = fixture.contains("diesel = { version = \"=2.2.6\"");
    let documents_probe = documentation.contains("## Diesel workflow");

    // Assert
    assert!(enables_probe);
    assert!(invokes_fixture);
    assert!(records_client_version);
    assert!(records_failed_probe);
    assert!(pins_diesel);
    assert!(documents_probe);
}
