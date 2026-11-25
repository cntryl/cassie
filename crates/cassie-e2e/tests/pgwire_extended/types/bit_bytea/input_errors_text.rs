//! Native BYTEA invalid-input phases, SQLSTATE and messages.
use crate::support_bind_planning as planning;
use crate::support_pgwire as wire;

#[test]
fn should_reject_short_octal_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\0' AS TEXT)::BYTEA", "22P02")];
    // Act
    let batches = planning::failures("byteainput-error-text-short_octal", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "invalid input syntax for type bytea"));
    }
}

#[test]
fn should_reject_long_octal_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\400' AS TEXT)::BYTEA", "22P02")];
    // Act
    let batches = planning::failures("byteainput-error-text-long_octal", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "invalid input syntax for type bytea"));
    }
}

#[test]
fn should_reject_bad_octal_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\378' AS TEXT)::BYTEA", "22P02")];
    // Act
    let batches = planning::failures("byteainput-error-text-bad_octal", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "invalid input syntax for type bytea"));
    }
}

#[test]
fn should_reject_bare_slash_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\' AS TEXT)::BYTEA", "22P02")];
    // Act
    let batches = planning::failures("byteainput-error-text-bare_slash", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "invalid input syntax for type bytea"));
    }
}

#[test]
fn should_reject_upper_x_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\X00' AS TEXT)::BYTEA", "22P02")];
    // Act
    let batches = planning::failures("byteainput-error-text-upper_x", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "invalid input syntax for type bytea"));
    }
}

#[test]
fn should_reject_mixed_prefix_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('a\\x00' AS TEXT)::BYTEA", "22P02")];
    // Act
    let batches = planning::failures("byteainput-error-text-mixed_prefix", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "invalid input syntax for type bytea"));
    }
}

#[test]
fn should_reject_bad_hex_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\xGG' AS TEXT)::BYTEA", "22023")];
    // Act
    let batches = planning::failures("byteainput-error-text-bad_hex", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "invalid hexadecimal digit: \"G\""));
    }
}

#[test]
fn should_reject_odd_hex_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\x0' AS TEXT)::BYTEA", "22023")];
    // Act
    let batches = planning::failures("byteainput-error-text-odd_hex", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(fields.iter().any(|(field, value)| *field == 'M'
            && value == "invalid hexadecimal data: odd number of digits"));
    }
}

#[test]
fn should_reject_odd_spaced_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\x 0' AS TEXT)::BYTEA", "22023")];
    // Act
    let batches = planning::failures("byteainput-error-text-odd_spaced", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(fields.iter().any(|(field, value)| *field == 'M'
            && value == "invalid hexadecimal data: odd number of digits"));
    }
}

#[test]
fn should_reject_split_pair_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\x0 0' AS TEXT)::BYTEA", "22023")];
    // Act
    let batches = planning::failures("byteainput-error-text-split_pair", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "invalid hexadecimal digit: \" \""));
    }
}

#[test]
fn should_reject_ff_hex_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\x\u{c}00' AS TEXT)::BYTEA", "22023")];
    // Act
    let batches = planning::failures("byteainput-error-text-ff_hex", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M'
                    && value == "invalid hexadecimal digit: \"\u{c}\"")
        );
    }
}

#[test]
fn should_reject_vt_hex_given_text_input_when_native_bind_conversion_fails() {
    // Arrange
    let cases = [("SELECT CAST('\\x\u{b}00' AS TEXT)::BYTEA", "22023")];
    // Act
    let batches = planning::failures("byteainput-error-text-vt_hex", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M'
                    && value == "invalid hexadecimal digit: \"\u{b}\"")
        );
    }
}
