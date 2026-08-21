#[test]
fn should_not_enforce_relative_thresholds_from_untrusted_rows() {
    // Arrange
    let gates = include_str!("../benches/support/stress_relative_gates.rs");

    // Act
    let checks_candidate_trust = gates.contains("!candidate.is_gate()");
    let checks_baseline_trust = gates.contains("!baseline.is_gate()");

    // Assert
    assert!(checks_candidate_trust);
    assert!(checks_baseline_trust);
}
