//! Compact JSON output parity, interruption and ownership probes.
use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

#[test]
fn should_match_serde_compact_output_for_nested_values_with_all_escapes() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let controls_text: String = (0_u8..=31).map(char::from).collect();
    let numbers: serde_json::Value =
        serde_json::from_str("[0,-0,1.00,1e200,123456789012345678901234567890.125]")
            .expect("numbers");
    let values = [
        serde_json::Value::Null,
        serde_json::json!([]),
        serde_json::json!({}),
        numbers,
        serde_json::json!({"z":[true, null, {"λ":"\"\\/Ω😀"}], "a":controls_text}),
    ];

    // Act
    for value in &values {
        let expected = value.to_string();
        let actual = serialize(value, &controls).expect("compact output");
        assert_eq!(actual.as_str(), expected);
        assert!(controls.current_query_memory_bytes() >= actual.as_str().len());
        drop(actual);
    }

    // Assert
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_after_scanning_a_real_unescaped_string_chunk() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let value = serde_json::Value::String("λ".repeat(8192));
    let mut scanned = false;

    // Act
    let result = serialize_observing(&value, &controls, |progress| {
        if matches!(progress, Progress::Scan) {
            scanned = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(scanned);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_after_copying_a_real_number_chunk() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let value: serde_json::Value =
        serde_json::from_str(&format!("1.{}", "1".repeat(8192))).expect("number");
    let mut copied = false;

    // Act
    let result = serialize_observing(&value, &controls, |progress| {
        if matches!(progress, Progress::Copy) {
            copied = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(copied);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_both_output_buffers_after_cancellation_during_growth_copy() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let mut sink = Sink {
        text: String::new(),
        memory: controls.reserve_query_memory(0).expect("owner"),
        controls: &controls,
        count: None,
    };
    sink.write(&"x".repeat(4096), &mut |_| {})
        .expect("initial output");
    let original = controls.current_query_memory_bytes();
    let mut retained_during_move = 0;

    // Act
    let result = sink.write(&"y".repeat(4096), &mut |progress| {
        if matches!(progress, Progress::Copy) {
            retained_during_move = controls.current_query_memory_bytes();
            cancellation.cancel();
        }
    });

    // Assert
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert!(retained_during_move > original);
    assert_eq!(controls.current_query_memory_bytes(), original);
    drop(sink);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_frame_owners_after_cancellation_during_replacement_move() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let value = serde_json::Value::Null;
    let mut stack = Stack::new(&controls).expect("stack");
    stack
        .push(Frame::Value(&value), &mut |_| {})
        .expect("first");
    stack
        .push(Frame::Value(&value), &mut |_| {})
        .expect("second");
    assert_eq!(stack.frames.len(), stack.frames.capacity());
    let original = controls.current_query_memory_bytes();
    let mut moved = false;

    // Act
    let result = stack.push(Frame::Value(&value), &mut |progress| {
        if matches!(progress, Progress::FrameMove) {
            moved = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(moved);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), original);
    drop(stack);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_deep_output_with_enforce_measured_budget_boundary() {
    // Arrange
    let mut value = serde_json::json!({"escaped":"line\nΩ"});
    for _ in 0..80 {
        value = serde_json::Value::Array(vec![value]);
    }
    let expected = value.to_string();
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    drop(serialize(&value, &baseline).expect("measure output"));
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
        let result = serialize(&value, &controls);

        // Assert
        assert_eq!(result.is_ok(), accepted);
        if let Ok(output) = &result {
            assert_eq!(output.as_str(), expected);
        }
        drop(result);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_count_exact_compact_bytes_for_nested_values_with_all_escapes() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let escapes: String = (0_u8..=31).map(char::from).collect();
    let numbers: serde_json::Value =
        serde_json::from_str("[1.00,1E200,123456789012345678901234567890.125]").expect("numbers");
    let values = [
        serde_json::Value::Null,
        serde_json::json!([]),
        serde_json::json!({}),
        numbers,
        serde_json::json!({"z":[true, null, {"λ":"\"\\/Ω😀"}], "a":escapes}),
    ];

    // Act / Assert
    for value in &values {
        // Assert
        assert_eq!(
            count(value, &controls).expect("size"),
            value.to_string().len()
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_cancel_during_a_real_counted_string_scan() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let value = serde_json::Value::String("λ".repeat(8192));
    let mut scanned = false;

    // Act
    let result = count_observing(&value, &controls, |progress| {
        if matches!(progress, Progress::Scan) {
            scanned = true;
            cancellation.cancel();
        }
    });

    // Assert
    assert!(scanned);
    assert!(matches!(result, Err(CassieError::QueryCanceled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_enforce_counting_frame_budget_without_allocating_output() {
    // Arrange
    let mut value = serde_json::Value::String("x".repeat(8192));
    for _ in 0..80 {
        value = serde_json::Value::Array(vec![value]);
    }
    let expected = value.to_string().len();
    let baseline =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    assert_eq!(count(&value, &baseline).expect("measure"), expected);
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
        let result = count(&value, &controls);
        // Assert
        assert_eq!(result.is_ok(), accepted);
        if let Ok(size) = result {
            assert_eq!(size, expected);
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
