//! Differential and resource probes for candidate NUMERIC tie formatting.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
use crate::types::extended::numeric::{self, Numeric};
use std::time::Instant;

#[test]
fn should_match_numeric_display_across_sign_scale_carry_boundaries() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut values = [
        "0",
        "-0.00",
        "1.2300",
        "0.00120",
        "-123456789.00001",
        "1e4000",
        "0e-16383",
    ]
    .into_iter()
    .map(|text| numeric::parse_text(text).expect("numeric input"))
    .collect::<Vec<_>>();
    values.push(Numeric::finite(num_bigint::BigInt::from(u128::MAX), 0));
    values.extend([Numeric::NaN, Numeric::Infinity, Numeric::NegInfinity]);

    // Act
    let results = values
        .iter()
        .map(|value| {
            let expected = value.to_string();
            let actual = format(value, &controls).expect("controlled numeric");
            (expected, actual)
        })
        .collect::<Vec<_>>();

    // Assert
    for (expected, actual) in results {
        assert_eq!(actual.text(), expected);
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_numeric_conversion_at_the_measured_budget_boundary() {
    // Arrange
    let value = numeric::parse_text("123456789012345678901234567890.0000").expect("numeric");
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    drop(format(&value, &baseline).expect("measure"));
    let required = baseline.peak_query_memory_bytes();
    assert!(required > 0);

    for (budget, accepted) in [(required - 1, false), (required, true)] {
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: budget,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );

        // Act
        let result = format(&value, &controls);

        // Assert
        assert_eq!(result.is_ok(), accepted);
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_during_numeric_limb_arithmetic() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let value = numeric::parse_text(&"7".repeat(1000)).expect("wide numeric");
    let mut steps = 0;

    // Act
    let result = format_observing(&value, &controls, |phase| {
        if matches!(phase, NumericPhase::Arithmetic) {
            steps += 1;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(steps, 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_during_numeric_tie_output() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let value = numeric::parse_text("1e4000").expect("wide numeric");
    let mut chunks = 0;

    // Act
    let result = format_observing(&value, &controls, |phase| {
        if matches!(phase, NumericPhase::Output) {
            chunks += 1;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::QueryCancelled))
    ));
    assert_eq!(chunks, 1);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_unaffordable_public_scale_before_output_allocation() {
    // Arrange
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let value = Numeric::Finite {
        mantissa: 1.into(),
        scale: u32::MAX,
    };
    // Act
    let result = format(&value, &controls);
    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
            _
        )))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_display_for_public_mantissas_across_scale_boundaries() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let coefficients = [
        num_bigint::BigInt::from(0),
        num_bigint::BigInt::from(1),
        num_bigint::BigInt::from(-1),
        num_bigint::BigInt::from(u128::MAX),
        -num_bigint::BigInt::from(u128::MAX),
    ];

    // Act / Assert
    for mantissa in &coefficients {
        for scale in [0, 1, 38, 39, 100] {
            let value = Numeric::Finite {
                mantissa: mantissa.clone(),
                scale,
            };
            let result = format(&value, &controls).expect("controlled format");
            assert_eq!(result.text(), value.to_string());
        }
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_integer_format_error_mapping_for_non_power_errors() {
    // Arrange
    let cancelled = crate::executor::QueryError::General("query canceled".into());
    let resource = crate::executor::QueryError::Cassie(crate::app::CassieError::ResourceLimit(
        "budget".into(),
    ));
    let power = crate::executor::QueryError::FloatPowerUnderflow;

    // Act / Assert
    assert!(
        matches!(super::format_integer_error(cancelled), crate::app::CassieError::Execution(ref message) if message == "query canceled")
    );
    assert!(
        matches!(super::format_integer_error(resource), crate::app::CassieError::ResourceLimit(ref message) if message == "budget")
    );
    assert!(
        matches!(super::format_integer_error(power), crate::app::CassieError::TypeInput(ref error) if error.sqlstate == "22003" && error.message == "value out of range: underflow")
    );
}
