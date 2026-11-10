use std::time::Instant;

use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};

fn frame(weight: i16, sign: u16, scale: u16, digits: &[u16]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&u16::try_from(digits.len()).expect("count").to_be_bytes());
    bytes.extend_from_slice(&weight.to_be_bytes());
    bytes.extend_from_slice(&sign.to_be_bytes());
    bytes.extend_from_slice(&scale.to_be_bytes());
    for digit in digits {
        bytes.extend_from_slice(&digit.to_be_bytes());
    }
    bytes
}

#[test]
fn should_match_binary_numeric_conversion_with_validation_order() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let cases = [
        frame(0, 0, 0, &[]),
        frame(0, 0, 4, &[1, 2300]),
        frame(1, 0, 4, &[0, 0, 1, 2345]),
        frame(-1, 0x4000, 2, &[1]),
        frame(1, 0x4000, 3, &[0, 0, 0]),
        frame(0, 0x4000, 0, &[123]),
        frame(0, 0xc000, u16::MAX, &[u16::MAX]),
        frame(0, 0xd000, u16::MAX, &[u16::MAX]),
        frame(0, 0xf000, u16::MAX, &[u16::MAX]),
    ];
    // Act / Assert
    for bytes in cases {
        let original =
            super::super::super::super::codecs::binary_to_value(&bytes, 1700).expect("original");
        let expected = super::super::super::super::codecs::value_to_text(&original, 1700);
        let (actual, memory) = super::decode(1700, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(actual, expected);
        drop((actual, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
    for bytes in [
        vec![],
        vec![0; 7],
        frame(0, 0x2000, 0, &[]),
        frame(0, 0, 16_384, &[]),
        frame(0, 0, 0, &[10_000]),
        {
            let mut bytes = frame(0, 0, 0, &[1]);
            bytes.pop();
            bytes
        },
    ] {
        let original =
            super::super::super::super::codecs::binary_to_value(&bytes, 1700).expect_err("invalid");
        let result = super::decode(1700, &bytes, 2, &controls);
        assert!(
            matches!(result, Err(CassieError::Parse(message)) if message == format!("COPY binary field 3 is invalid: {original}"))
        );
    }
}

#[test]
fn should_bound_binary_numeric_zero_weight_output_with_overlap() {
    // Arrange
    let bytes = frame(i16::MAX, 0x4000, 0, &[0]);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 11,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(bytes.len()).expect("input");
    // Act
    let (actual, memory) = super::decode(1700, &bytes, 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "0");
    assert_eq!(controls.current_query_memory_bytes(), 11);
    drop((actual, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_interrupt_binary_numeric_group_scan_with_release() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let bytes = frame(0, 0, 0, &[0; 513]);
    // Act
    let result = super::decode_observed(1700, &bytes, 0, &controls, || cancellation.cancel());
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_deny_large_binary_numeric_output_before_allocation() {
    // Arrange
    let bytes = frame(i16::MAX, 0, 0, &[1]);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 128,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(bytes.len()).expect("input");
    // Act
    let result = super::decode(1700, &bytes, 0, &controls);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), bytes.len());
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_zero_with_extreme_negative_weight_with_scale() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let bytes = frame(i16::MIN, 0x4000, 2, &[]);
    // Act
    let (actual, memory) = super::decode(1700, &bytes, 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "0.00");
    drop((actual, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_zero_with_extreme_positive_weight_with_scale() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let bytes = frame(i16::MAX, 0x4000, 2, &[0, 0]);
    // Act
    let (actual, memory) = super::decode(1700, &bytes, 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "0.00");
    drop((actual, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_bounded_binary_numeric_group_combinations() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let mut seed = 0x12_34_56_78_u32;
    // Act / Assert
    for case in 0..256 {
        let mut digits = [0_u16; 5];
        for digit in &mut digits {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *digit = u16::try_from(seed % 10_000).expect("digit");
        }
        let count = case % 6;
        let weight = i16::try_from(case % 13).expect("weight") - 6;
        let scale = u16::try_from(case % 13).expect("scale");
        let sign = if case % 2 == 0 { 0 } else { 0x4000 };
        let bytes = frame(weight, sign, scale, &digits[..count]);
        let original =
            super::super::super::super::codecs::binary_to_value(&bytes, 1700).expect("original");
        let expected = super::super::super::super::codecs::value_to_text(&original, 1700);
        let (actual, memory) = super::decode(1700, &bytes, 0, &controls)
            .expect("decode")
            .expect("handled");
        // Assert
        assert_eq!(actual, expected, "case {case}");
        drop((actual, memory));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_release_binary_numeric_output_when_cancelled_during_emission() {
    // Arrange
    let bytes = frame(300, 0, 0, &[1]);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    // Act
    let result = super::decode_observed(1700, &bytes, 0, &controls, || cancellation.cancel());
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_binary_numeric_output_when_cancelled_during_fraction() {
    // Arrange
    let bytes = frame(0, 0, 1000, &[]);
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    // Act
    let result = super::decode_observed(1700, &bytes, 0, &controls, || cancellation.cancel());
    // Assert
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert!(controls.peak_query_memory_bytes() > 0);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_scaled_binary_numeric_at_exact_input_output_budget() {
    // Arrange
    let bytes = frame(0, 0, 4, &[1, 2300]);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 18,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = controls.reserve_query_memory(bytes.len()).expect("input");
    // Act
    let (actual, memory) = super::decode(1700, &bytes, 0, &controls)
        .expect("decode")
        .expect("handled");
    // Assert
    assert_eq!(actual, "1.2300");
    assert_eq!(controls.current_query_memory_bytes(), 18);
    drop((actual, memory));
    drop(input);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 17,
        ..CassieRuntimeLimits::default()
    };
    let denied = QueryExecutionControls::from_limits(&limits, Instant::now());
    let input = denied.reserve_query_memory(bytes.len()).expect("input");
    assert!(matches!(
        super::decode(1700, &bytes, 0, &denied),
        Err(CassieError::ResourceLimit(_))
    ));
    assert_eq!(denied.current_query_memory_bytes(), bytes.len());
    drop(input);
    assert_eq!(denied.current_query_memory_bytes(), 0);
}
