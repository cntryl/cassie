//! Typed power failures are folded without broadening other math diagnostics.
use super::*;

#[test]
fn should_fold_only_the_typed_power_range_variants() {
    // Arrange
    let overflow = QueryError::FloatPowerOverflow;
    let underflow = QueryError::FloatPowerUnderflow;
    let unrelated = QueryError::Cassie(crate::app::CassieError::TypeInput(
        crate::types::extended::TypeInputError::new("22003", "value out of range: overflow".into()),
    ));
    let float_input = QueryError::Cassie(crate::app::CassieError::FloatInput(
        crate::types::FloatInputError::Range,
    ));

    // Act / Assert
    assert!(is_folding_error(&overflow));
    assert!(is_folding_error(&underflow));
    assert!(!is_folding_error(&unrelated));
    assert!(is_folding_error(&float_input));
}
