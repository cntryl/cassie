//! Native error phases and Sync recovery for invalid size text.
use crate::support_bind_planning as planning;
use crate::support_pgwire as wire;

#[test]
fn should_reject_bytes_invalid_0_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_0", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_1_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('x')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_1", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"x\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_2_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('NaN')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_2", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"NaN\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_3_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('Infinity')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_3", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"Infinity\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_4_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('-Infinity')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_4", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"-Infinity\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_5_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('1KiB')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_5", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"1KiB\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_6_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('1 EB')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_6", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"1 EB\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_7_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('1 k B')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_7", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"1 k B\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_8_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('1,5 MB')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_8", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"1,5 MB\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_9_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('1e')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_9", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"1e\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_10_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('1e+')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_10", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"1e+\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_11_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('--1')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_11", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"--1\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_12_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('+.')", "22023")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_12", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields
                .iter()
                .any(|(field, value)| *field == 'M' && value == "invalid size: \"+.\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_13_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('1e 2 kB')", "22P02")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_13", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields.iter().any(|(field, value)| *field == 'M'
                && value == "invalid input syntax for type numeric: \"1e 2\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_14_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('1e\t+2 kB')", "22P02")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_14", cases);
    // Assert
    planning::assert_failures(&batches, cases);
    for cycle in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&cycle[2][0].1);
        assert!(
            fields.iter().any(|(field, value)| *field == 'M'
                && value == "invalid input syntax for type numeric: \"1e\t+2\""),
            "{fields:?}"
        );
    }
}

#[test]
fn should_reject_bytes_invalid_15_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('1e1000000')", "22003")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_15", cases);
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
fn should_reject_bytes_invalid_16_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[("SELECT pg_catalog.pg_size_bytes('1e-100000')", "22003")];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_16", cases);
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
fn should_reject_bytes_invalid_17_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[(
        "SELECT pg_catalog.pg_size_bytes('9223372036854775808')",
        "22003",
    )];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_17", cases);
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
fn should_reject_bytes_invalid_18_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[(
        "SELECT pg_catalog.pg_size_bytes('-9223372036854775809')",
        "22003",
    )];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_18", cases);
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
fn should_reject_bytes_invalid_19_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[(
        "SELECT pg_catalog.pg_size_bytes('9223372036854775807.5')",
        "22003",
    )];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_19", cases);
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
fn should_reject_bytes_invalid_20_given_invalid_size_text_when_bind_is_attempted() {
    // Arrange
    let cases = &[(
        "SELECT pg_catalog.pg_size_bytes('-9223372036854775808.5')",
        "22003",
    )];
    // Act
    let batches = planning::failures("catalog-bytes_invalid_20", cases);
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
