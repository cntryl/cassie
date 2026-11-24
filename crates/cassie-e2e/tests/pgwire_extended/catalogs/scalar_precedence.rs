//! Adversarial numeric range, unit precedence and exact rounding witnesses.
use crate::support_bind_planning as planning;
use crate::support_pgwire as wire;
use crate::support_values_casts;

#[test]
fn should_preserve_precedence_0_given_numeric_range_input_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_size_bytes('1e131072 bogus') AS output", "22003")];
    // Act
    let batches = planning::failures("catalog-precedence-0", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "value overflows numeric format"),
            "{fields:?}"
        );
    }
}

#[test]
fn should_preserve_precedence_1_given_numeric_range_input_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_size_bytes('1e131071 bogus') AS output", "22023")];
    // Act
    let batches = planning::failures("catalog-precedence-1", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"1e131071 bogus\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_preserve_precedence_2_given_numeric_range_input_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_size_bytes('1e-16384 bogus') AS output", "22003")];
    // Act
    let batches = planning::failures("catalog-precedence-2", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "value overflows numeric format"),
            "{fields:?}"
        );
    }
}

#[test]
fn should_preserve_precedence_3_given_numeric_range_input_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_size_bytes('1e-16383 bogus') AS output", "22023")];
    // Act
    let batches = planning::failures("catalog-precedence-3", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"1e-16383 bogus\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_preserve_precedence_4_given_numeric_range_input_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_size_bytes('1e131072 B') AS output", "22003")];
    // Act
    let batches = planning::failures("catalog-precedence-4", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "value overflows numeric format"),
            "{fields:?}"
        );
    }
}

#[test]
fn should_preserve_precedence_5_given_numeric_range_input_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_size_bytes('1e131071 B') AS output", "22003")];
    // Act
    let batches = planning::failures("catalog-precedence-5", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "bigint out of range"),
            "{fields:?}"
        );
    }
}

#[test]
fn should_preserve_precision_6_given_extreme_decimal_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_size_bytes('0e1000000') AS output";
    // Act
    let batches = planning::pruned("catalog-precision-6", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_precision_7_given_extreme_decimal_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_size_bytes('0e1073741823') AS output";
    // Act
    let batches = planning::pruned("catalog-precision-7", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_precedence_8_given_numeric_range_input_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_size_bytes('0e1073741824') AS output", "22003")];
    // Act
    let batches = planning::failures("catalog-precedence-8", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "value overflows numeric format"),
            "{fields:?}"
        );
    }
}

#[test]
fn should_preserve_precedence_9_given_numeric_range_input_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_size_bytes('0e-16384') AS output", "22003")];
    // Act
    let batches = planning::failures("catalog-precedence-9", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "value overflows numeric format"),
            "{fields:?}"
        );
    }
}

#[test]
fn should_preserve_precision_10_given_extreme_decimal_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_size_bytes('0.00000000000000000000000000000000001e35 B') AS output";
    // Act
    let batches = planning::pruned("catalog-precision-10", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_precision_11_given_extreme_decimal_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_size_bytes('0.000488281250000000000000000000001 kB') AS output";
    // Act
    let batches = planning::pruned("catalog-precision-11", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 49]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 1]],
        ],
    );
}

#[test]
fn should_preserve_precision_12_given_extreme_decimal_input_when_wire_formats_are_crossed() {
    // Arrange
    let sql = "SELECT pg_size_bytes('-0.000488281249999999999999999999999 kB') AS output";
    // Act
    let batches = planning::pruned("catalog-precision-12", sql);
    // Assert
    support_values_casts::assert_result(
        &batches,
        ("output", 20, 8, -1),
        [
            &[&[0, 1, 0, 0, 0, 1, 48]],
            &[&[0, 1, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0, 0]],
        ],
    );
}

#[test]
fn should_preserve_precedence_13_given_numeric_range_input_when_bind_is_attempted() {
    // Arrange
    let cases = &[(
        "SELECT pg_size_bytes('1e999999999999999999999999999999999999999 bogus') AS output",
        "22003",
    )];
    // Act
    let batches = planning::failures("catalog-precedence-13", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "value overflows numeric format"),
            "{fields:?}"
        );
    }
}
