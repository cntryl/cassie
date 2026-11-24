//! Native signed-domain and input-priority witnesses for the shared kernel.
use super::{
    arithmetic, parse, IntegerArithmeticError, IntegerInputError, IntegerOperation, IntegerWidth,
};

#[test]
fn should_parse_signed_extremes_given_existing_widths_when_radix_input_is_received() {
    // Arrange
    let cases = [
        (IntegerWidth::I16, "-0x_8000", -32_768),
        (IntegerWidth::I16, "0o_77777", 32_767),
        (IntegerWidth::I32, "-0x8000_0000", -2_147_483_648),
        (IntegerWidth::I32, " \t+2_147_483_647\r\n", 2_147_483_647),
        (IntegerWidth::I64, "-0x8000_0000_0000_0000", i64::MIN),
        (IntegerWidth::I64, "0x7fff_ffff_ffff_ffff", i64::MAX),
        (
            IntegerWidth::I64,
            "9_007_199_254_740_993",
            9_007_199_254_740_993,
        ),
    ];
    // Act
    let outcomes = cases.map(|(width, input, expected)| (parse(input, width), expected));
    // Assert
    for (outcome, expected) in outcomes {
        assert_eq!(outcome, Ok(expected));
    }
}

#[test]
fn should_preserve_native_error_priority_given_bad_suffixes_when_accumulation_overflows() {
    // Arrange
    let cases = [
        (IntegerWidth::I16, "00032768x", IntegerInputError::Syntax),
        (IntegerWidth::I16, "327690x", IntegerInputError::Range),
        (IntegerWidth::I16, "0x8000x", IntegerInputError::Syntax),
        (IntegerWidth::I16, "0x80000x", IntegerInputError::Range),
        (IntegerWidth::I16, "0o100000x", IntegerInputError::Syntax),
        (IntegerWidth::I16, "0o1000000x", IntegerInputError::Range),
        (
            IntegerWidth::I64,
            "9223372036854775808x",
            IntegerInputError::Syntax,
        ),
        (
            IntegerWidth::I64,
            "-9223372036854775809x",
            IntegerInputError::Syntax,
        ),
        (
            IntegerWidth::I64,
            "999999999999999999999x",
            IntegerInputError::Range,
        ),
    ];
    // Act
    let outcomes = cases.map(|(width, input, expected)| (parse(input, width), expected));
    // Assert
    for (outcome, expected) in outcomes {
        assert_eq!(outcome, Err(expected));
    }
}

#[test]
fn should_reject_noninteger_text_given_invalid_separators_when_scalar_input_is_parsed() {
    // Arrange
    let inputs = [
        "",
        "+",
        "_1",
        "0x_",
        "0x__1",
        "1__0",
        "1_",
        "1.0",
        "1e3",
        "\u{a0}42\u{a0}",
    ];
    // Act
    let outcomes = inputs.map(|input| parse(input, IntegerWidth::I64));
    // Assert
    for outcome in outcomes {
        assert_eq!(outcome, Err(IntegerInputError::Syntax));
    }
}

#[test]
fn should_preserve_exact_integer_results_given_typed_operands_when_arithmetic_executes() {
    // Arrange
    let cases = [
        (IntegerWidth::I16, IntegerOperation::Add, 1, 2, 3),
        (IntegerWidth::I16, IntegerOperation::Mul, 3, 4, 12),
        (IntegerWidth::I32, IntegerOperation::Div, -5, 2, -2),
        (
            IntegerWidth::I64,
            IntegerOperation::Add,
            9_007_199_254_740_993,
            2,
            9_007_199_254_740_995,
        ),
    ];
    // Act
    let outcomes = cases.map(|(width, operation, left, right, expected)| {
        (arithmetic(width, operation, left, right), expected)
    });
    // Assert
    for (outcome, expected) in outcomes {
        assert_eq!(outcome, Ok(expected));
    }
}

#[test]
fn should_enforce_operator_domains_given_boundary_operands_when_arithmetic_overflows() {
    // Arrange
    let cases = [
        (IntegerWidth::I16, IntegerOperation::Add, 32_767, 1),
        (IntegerWidth::I32, IntegerOperation::Add, 2_147_483_647, 1),
        (IntegerWidth::I32, IntegerOperation::Div, -2_147_483_648, -1),
        (IntegerWidth::I64, IntegerOperation::Add, i64::MAX, 1),
        (IntegerWidth::I64, IntegerOperation::Div, i64::MIN, -1),
        (IntegerWidth::I16, IntegerOperation::Sub, 50_000, 50_000),
    ];
    // Act
    let outcomes =
        cases.map(|(width, operation, left, right)| arithmetic(width, operation, left, right));
    // Assert
    for outcome in outcomes {
        assert_eq!(outcome, Err(IntegerArithmeticError::Range));
    }
}

#[test]
fn should_reject_zero_divisors_given_each_existing_width_when_integer_division_executes() {
    // Arrange
    let widths = [IntegerWidth::I16, IntegerWidth::I32, IntegerWidth::I64];
    // Act
    let outcomes = widths.map(|width| arithmetic(width, IntegerOperation::Div, 1, 0));
    // Assert
    for outcome in outcomes {
        assert_eq!(outcome, Err(IntegerArithmeticError::DivisionByZero));
    }
}

#[test]
fn should_check_signed_negation_given_each_width_when_minimum_is_negated() {
    // Arrange
    let widths = [IntegerWidth::I16, IntegerWidth::I32, IntegerWidth::I64];
    // Act
    let outcomes = widths.map(|width| {
        (
            super::negate(width, width.bounds().0),
            super::negate(width, -1),
        )
    });
    // Assert
    for (minimum, ordinary) in outcomes {
        assert_eq!(minimum, Err(IntegerArithmeticError::Range));
        assert_eq!(ordinary, Ok(1));
    }
}
