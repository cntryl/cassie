use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::Instant;

fn controls(budget: usize) -> QueryExecutionControls {
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: budget,
        ..CassieRuntimeLimits::default()
    };
    QueryExecutionControls::from_limits(&limits, Instant::now())
}
fn key(text: &str, controls: &QueryExecutionControls) -> JsonbKey {
    JsonbKey::from_text(text, Some(controls)).expect("valid admitted JSONB key")
}
fn hash(key: &JsonbKey) -> u64 {
    let mut state = DefaultHasher::new();
    key.hash(&mut state);
    state.finish()
}

#[test]
fn should_share_exact_jsonb_numeric_identity_across_key_operations() {
    // Arrange
    let controls = controls(16_777_216);
    let equivalent = ["1", "1.0", "1e0", "0.0100e2"];
    // Act
    let keys = equivalent.map(|text| key(text, &controls));
    // Assert
    for left in &keys {
        for right in &keys {
            assert_eq!(left, right);
            assert_eq!(left.cmp(right), Ordering::Equal);
            assert_eq!(hash(left), hash(right));
        }
    }
    assert!(
        key("9007199254740993.0000000000000001", &controls) > key("9007199254740993", &controls)
    );
    assert_eq!(key("-0.000e100", &controls), key("0", &controls));
}

#[test]
fn should_match_the_pinned_jsonb_type_order() {
    // Arrange
    let controls = controls(16_777_216);
    let corpus = [
        "[]",
        "null",
        "\"x\"",
        "2",
        "10",
        "false",
        "[1]",
        "[0,0]",
        "{}",
        "{\"aa\":0}",
        "{\"b\":1}",
    ];
    // Act
    let keys = corpus.map(|text| key(text, &controls));
    // Assert
    for (index, left) in keys.iter().enumerate() {
        for (right_index, right) in keys.iter().enumerate() {
            assert_eq!(left.cmp(right), index.cmp(&right_index));
        }
    }
    assert!(key("[[]]", &controls) > key("[null]", &controls));
}

#[test]
fn should_retain_jsonb_key_memory_until_its_last_clone_drops() {
    // Arrange
    let controls = controls(16_777_216);
    // Act
    let original = key("{\"n\":123456789.123400}", &controls);
    let live = controls.current_query_memory_bytes();
    let clone = original.clone();
    drop(original);
    let cloned_live = controls.current_query_memory_bytes();
    drop(clone);
    // Assert
    assert!(live > 0);
    assert_eq!(live, cloned_live);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_jsonb_key_construction_at_its_measured_boundary() {
    // Arrange
    let ample = controls(16_777_216);
    let document = "{\"a\":[1.0,2,null],\"b\":true}";
    let measured = key(document, &ample);
    let peak = ample.peak_query_memory_bytes();
    drop(measured);
    let exact = controls(peak);
    let scarce = controls(peak - 1);
    // Act
    let admitted = JsonbKey::from_text(document, Some(&exact));
    let denied = JsonbKey::from_text(document, Some(&scarce));
    // Assert
    assert!(admitted.is_ok());
    assert!(matches!(denied, Err(CassieError::ResourceLimit(_))));
    drop(admitted);
    assert_eq!(exact.current_query_memory_bytes(), 0);
    assert_eq!(scarce.current_query_memory_bytes(), 0);
}

#[test]
fn should_cancel_jsonb_normalization_after_actual_token_work() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let value: serde_json::Value = serde_json::from_str("[1,2]").expect("document");
    let mut observed = 0;
    // Act
    let result = JsonbKey::from_json_observing(&value, Some(&controls), || {
        observed += 1;
        cancellation.cancel();
    });
    // Assert
    assert!(observed > 0);
    assert!(matches!(result, Err(CassieError::QueryCancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_validate_jsonb_numeric_format_without_dense_exponent_expansion() {
    // Arrange
    let controls = controls(16_777_216);
    // Act
    let large = JsonbKey::from_text("1e131071", Some(&controls));
    let zero = JsonbKey::from_text("0e1073741823", Some(&controls));
    let too_large = JsonbKey::from_text("1e131072", Some(&controls));
    let too_small = JsonbKey::from_text("1e-16384", Some(&controls));
    // Assert
    assert!(large.is_ok());
    assert!(zero.is_ok());
    assert!(too_large.is_err());
    assert!(too_small.is_err());
}

#[test]
fn should_order_sparse_jsonb_coefficients_across_signed_decimal_weights() {
    // Arrange
    let controls = controls(16_777_216);
    let corpus = [
        "-1e131071",
        "-100",
        "-1.2",
        "-0.1",
        "-0.01",
        "-1e-16383",
        "0",
        "1e-16383",
        "0.01",
        "0.1",
        "1.02",
        "1.2",
        "100",
        "1e131071",
    ];
    // Act
    let keys = corpus.map(|text| key(text, &controls));
    // Assert
    for (index, left) in keys.iter().enumerate() {
        for (other, right) in keys.iter().enumerate() {
            assert_eq!(
                left.cmp(right),
                index.cmp(&other),
                "{} vs {}",
                corpus[index],
                corpus[other]
            );
        }
    }
}

#[test]
fn should_release_jsonb_workspace_after_a_late_invalid_numeric_token() {
    // Arrange
    let controls = controls(16_777_216);
    // Act
    let result = JsonbKey::from_text(r#"[{"ok":1},1e-16384]"#, Some(&controls));
    // Assert
    assert!(matches!(result,Err(CassieError::TypeInput(ref error)) if error.sqlstate=="22003"));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
