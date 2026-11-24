use cassie::config::{CassieRuntimeLimits, OperatorSwitchingEnabled};

#[test]
fn should_keep_adaptive_controls_disabled_by_default() {
    // Arrange
    let limits = CassieRuntimeLimits::default();

    // Act
    let adaptive_enabled = limits.adaptive_execution_enabled;
    let operator_switching_enabled = limits.operator_switching_enabled;

    // Assert
    assert!(!adaptive_enabled);
    assert_eq!(
        operator_switching_enabled,
        OperatorSwitchingEnabled::disabled()
    );
}
