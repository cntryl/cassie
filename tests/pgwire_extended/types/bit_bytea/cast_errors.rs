//! Native overlength BYTEA casts fail during Bind planning and recover.
use crate::support_bind_planning as planning;
use crate::support_pgwire as wire;

#[test]
fn should_reject_int2_zero_scalar_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [("SELECT '\\x000000'::BYTEA::INT2 AS output", "22003")];
    // Act
    let batches = planning::failures("byteacasts-error-int2_zero_scalar", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "smallint out of range"));
    }
}

#[test]
fn should_reject_int2_zero_cte_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\x000000'::BYTEA)) SELECT v::INT2 AS output FROM q",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int2_zero_cte", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "smallint out of range"));
    }
}

#[test]
fn should_reject_int2_ones_scalar_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [("SELECT '\\xffffff'::BYTEA::INT2 AS output", "22003")];
    // Act
    let batches = planning::failures("byteacasts-error-int2_ones_scalar", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "smallint out of range"));
    }
}

#[test]
fn should_reject_int2_ones_cte_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\xffffff'::BYTEA)) SELECT v::INT2 AS output FROM q",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int2_ones_cte", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "smallint out of range"));
    }
}

#[test]
fn should_reject_int2_two_extra_scalar_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [("SELECT '\\x00000000'::BYTEA::INT2 AS output", "22003")];
    // Act
    let batches = planning::failures("byteacasts-error-int2_two_extra_scalar", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "smallint out of range"));
    }
}

#[test]
fn should_reject_int2_two_extra_cte_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\x00000000'::BYTEA)) SELECT v::INT2 AS output FROM q",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int2_two_extra_cte", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "smallint out of range"));
    }
}

#[test]
fn should_reject_int4_zero_scalar_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [("SELECT '\\x0000000000'::BYTEA::INT4 AS output", "22003")];
    // Act
    let batches = planning::failures("byteacasts-error-int4_zero_scalar", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "integer out of range"));
    }
}

#[test]
fn should_reject_int4_zero_cte_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\x0000000000'::BYTEA)) SELECT v::INT4 AS output FROM q",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int4_zero_cte", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "integer out of range"));
    }
}

#[test]
fn should_reject_int4_ones_scalar_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [("SELECT '\\xffffffffff'::BYTEA::INT4 AS output", "22003")];
    // Act
    let batches = planning::failures("byteacasts-error-int4_ones_scalar", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "integer out of range"));
    }
}

#[test]
fn should_reject_int4_ones_cte_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\xffffffffff'::BYTEA)) SELECT v::INT4 AS output FROM q",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int4_ones_cte", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "integer out of range"));
    }
}

#[test]
fn should_reject_int4_two_extra_scalar_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [("SELECT '\\x000000000000'::BYTEA::INT4 AS output", "22003")];
    // Act
    let batches = planning::failures("byteacasts-error-int4_two_extra_scalar", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "integer out of range"));
    }
}

#[test]
fn should_reject_int4_two_extra_cte_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\x000000000000'::BYTEA)) SELECT v::INT4 AS output FROM q",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int4_two_extra_cte", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "integer out of range"));
    }
}

#[test]
fn should_reject_int8_zero_scalar_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "SELECT '\\x000000000000000000'::BYTEA::INT8 AS output",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int8_zero_scalar", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "bigint out of range"));
    }
}

#[test]
fn should_reject_int8_zero_cte_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\x000000000000000000'::BYTEA)) SELECT v::INT8 AS output FROM q",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int8_zero_cte", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "bigint out of range"));
    }
}

#[test]
fn should_reject_int8_ones_scalar_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "SELECT '\\xffffffffffffffffff'::BYTEA::INT8 AS output",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int8_ones_scalar", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "bigint out of range"));
    }
}

#[test]
fn should_reject_int8_ones_cte_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\xffffffffffffffffff'::BYTEA)) SELECT v::INT8 AS output FROM q",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int8_ones_cte", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "bigint out of range"));
    }
}

#[test]
fn should_reject_int8_two_extra_scalar_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "SELECT '\\x00000000000000000000'::BYTEA::INT8 AS output",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int8_two_extra_scalar", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "bigint out of range"));
    }
}

#[test]
fn should_reject_int8_two_extra_cte_given_overlength_bytea_when_bind_planning_folds_the_cast() {
    // Arrange
    let cases = [(
        "WITH q(v) AS (VALUES ('\\x00000000000000000000'::BYTEA)) SELECT v::INT8 AS output FROM q",
        "22003",
    )];
    // Act
    let batches = planning::failures("byteacasts-error-int8_two_extra_cte", &cases);
    // Assert
    planning::assert_failures(&batches, &cases);
    for batch in batches.as_chunks::<7>().0 {
        let fields = wire::parse_error_fields(&batch[2][0].1);
        assert!(fields
            .iter()
            .any(|(field, value)| *field == 'M' && value == "bigint out of range"));
    }
}
