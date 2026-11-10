//! Controlled numeric literal grammar, exact radix bounds and interruption.
use super::*;
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn error_class(result: Result<(), ValidationError>) -> Result<(), &'static str> {
    result.map_err(|error| match error {
        ValidationError::Syntax => "22P02",
        ValidationError::Range => "22003",
        ValidationError::Interrupted(_) => "interrupted",
    })
}

#[test]
fn should_match_existing_numeric_input_grammar_with_error_precedence() {
    // Arrange
    let controls = controls(16 * 1024 * 1024);
    let inputs = [
        "\x0b1\x0b",
        "\x0b  -1.5  \x0b",
        "1\x0b2",
        "NaN",
        "+NaN",
        " -inf ",
        "+Infinity",
        ".5",
        "5.",
        "-.5",
        "+5.",
        "1_000.50",
        "1e1_0",
        "0x_FF",
        "0b_1010",
        "0o_17",
        "0x_",
        "0x__f",
        "0b2",
        "1._2",
        "1__0",
        "1e_2",
        "1e",
        "0e-16384",
        "0e20000",
        "1e131072",
        "1e-16384",
        "1__0e99999999999999999999",
        "1e999999999999999999x",
    ];

    // Act / Assert
    for input in inputs {
        let expected = crate::types::extended::numeric::parse_text(input)
            .map(|_| ())
            .map_err(|error| error.sqlstate);
        // Assert
        assert_eq!(
            error_class(validate(input, Some(&controls))),
            expected,
            "{input:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_enforce_exact_small_radix_with_decimal_digit_boundaries() {
    // Arrange
    let controls = controls(1_048_576);
    let limits = Limits {
        max_integer_digits: 3,
        max_dscale: 2,
    };
    let cases = [
        ("0x3e7", Ok(())),
        ("0x3e8", Err("22003")),
        ("0o1747", Ok(())),
        ("0o1750", Err("22003")),
        ("0b1111100111", Ok(())),
        ("0b1111101000", Err("22003")),
        ("000_999", Ok(())),
        ("1e3", Err("22003")),
        (".01", Ok(())),
        (".001", Err("22003")),
        ("0.00", Ok(())),
        ("0.000", Err("22003")),
        ("0xFFFF_FFFFg", Err("22P02")),
    ];

    // Act / Assert
    for (input, expected) in cases {
        // Assert
        assert_eq!(
            error_class(validate_with_limits_observing(
                input,
                limits,
                Some(&controls),
                |_| {}
            )),
            expected,
            "{input:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_check_the_real_radix_integer_limit_exactly() {
    // Arrange
    let controls = controls(64 * 1024 * 1024);
    let threshold = num_bigint::BigUint::from(10_u8).pow(131_072);
    let below = format!("0x{}", (&threshold - 1_u8).to_str_radix(16));
    let above = format!("0x{}", threshold.to_str_radix(16));

    // Act
    let accepted = validate(&below, Some(&controls));
    let rejected = validate(&above, Some(&controls));

    // Assert
    assert_eq!(error_class(accepted), Ok(()));
    assert_eq!(error_class(rejected), Err("22003"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_interrupt_radix_conversion_with_release_temporary_limbs() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let limits = Limits {
        max_integer_digits: 3,
        max_dscale: 2,
    };
    let mut steps = 0;

    // Act
    let result = validate_with_limits_observing("0x3e7", limits, Some(&controls), |phase| {
        if matches!(phase, ValidationPhase::RadixConversion) {
            steps += 1;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(ValidationError::Interrupted(CassieError::QueryCancelled))
    ));
    assert_eq!(steps, 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_radix_limbs_before_conversion() {
    // Arrange
    let denied = controls(std::mem::size_of::<u64>() - 1);
    let admitted = controls(1_048_576);
    let limits = Limits {
        max_integer_digits: 3,
        max_dscale: 2,
    };
    let mut denied_steps = 0;

    // Act
    let failure = validate_with_limits_observing("0x3e7", limits, Some(&denied), |phase| {
        if matches!(phase, ValidationPhase::RadixConversion) {
            denied_steps += 1;
        }
    });
    let success = validate_with_limits_observing("0x3e7", limits, Some(&admitted), |_| {});

    // Assert
    assert!(matches!(
        failure,
        Err(ValidationError::Interrupted(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(denied_steps, 0);
    assert!(success.is_ok());
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(admitted.current_query_memory_bytes(), 0);
}
