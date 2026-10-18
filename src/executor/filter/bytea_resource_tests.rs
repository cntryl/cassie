//! Operation-specific scratch boundaries use small native PostgreSQL witnesses.
use super::{evaluate, Value};
use crate::app::CassieError;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            ..CassieRuntimeLimits::default()
        },
        std::time::Instant::now(),
    )
}

#[test]
fn should_admit_scalar_bytea_reads_given_exact_decoded_budget_when_input_is_small() {
    // Arrange
    let input = Value::String(format!("\\x{}", "ff".repeat(16)));
    for (name, args, expected) in [
        ("get_byte", vec![input.clone(), Value::Int64(5)], 255),
        ("get_bit", vec![input.clone(), Value::Int64(5)], 1),
        ("bit_count", vec![input.clone()], 128),
    ] {
        let controls = controls(16);
        // Act
        let result = evaluate(name, &args, Some(&controls)).unwrap();
        // Assert
        assert_eq!(result.unwrap(), Value::Int64(expected), "{name}");
        assert_eq!(controls.peak_query_memory_bytes(), 16, "{name}");
        assert_eq!(controls.current_query_memory_bytes(), 0, "{name}");
    }
}

#[test]
fn should_reject_scalar_bytea_read_given_one_byte_short_budget_when_decode_is_admitted() {
    // Arrange
    let args = [
        Value::String(format!("\\x{}", "ff".repeat(16))),
        Value::Int64(5),
    ];
    let controls = controls(15);
    // Act
    let result = evaluate("get_byte", &args, Some(&controls))
        .unwrap()
        .map_err(CassieError::from);
    // Assert
    assert!(matches!(result, Err(CassieError::ResourceLimit(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_bytea_hash_given_fixed_digest_budget_when_input_is_small() {
    // Arrange
    let args = [Value::String(format!("\\x{}", "ff".repeat(16)))];
    let expected = "\\x5ac6a5945f16500911219129984ba8b387a06f24fe383ce4e81a73294065461b";
    let controls = controls(16 + 66);
    // Act
    let result = evaluate("sha256", &args, Some(&controls)).unwrap();
    // Assert
    assert_eq!(result.unwrap(), Value::String(expected.into()));
    assert_eq!(controls.peak_query_memory_bytes(), 82);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_bytea_encodings_given_format_output_budget_when_input_is_small() {
    // Arrange
    let input = Value::String(format!("\\x{}", "ff".repeat(16)));
    for (format, output, scratch) in [
        ("hex", "ff".repeat(16), 48),
        ("escape", "\\377".repeat(16), 80),
        ("base64", "/////////////////////w==".into(), 40),
    ] {
        let args = [input.clone(), Value::String(format.into())];
        let controls = controls(scratch);
        // Act
        let result = evaluate("encode", &args, Some(&controls)).unwrap();
        // Assert
        assert_eq!(result.unwrap(), Value::String(output), "{format}");
        assert_eq!(controls.peak_query_memory_bytes(), scratch, "{format}");
        assert_eq!(controls.current_query_memory_bytes(), 0, "{format}");
    }
}

#[test]
fn should_admit_bytea_decodings_given_input_and_carrier_budget_when_input_is_small() {
    // Arrange
    let expected = Value::String(format!("\\x{}", "ff".repeat(16)));
    for (format, input, scratch) in [
        ("hex", "ff".repeat(16), 50),
        ("escape", "\\377".repeat(16), 194),
        ("base64", "/////////////////////w==".into(), 56),
    ] {
        let args = [Value::String(input), Value::String(format.into())];
        let controls = controls(scratch);
        // Act
        let result = evaluate("decode", &args, Some(&controls)).unwrap();
        // Assert
        assert_eq!(result.unwrap(), expected, "{format}");
        assert_eq!(controls.peak_query_memory_bytes(), scratch, "{format}");
        assert_eq!(controls.current_query_memory_bytes(), 0, "{format}");
    }
}

#[test]
fn should_reject_invalid_format_error_given_one_byte_short_budget_when_codec_is_reached() {
    // Arrange
    let format = "bogus";
    let error_length = format!("unrecognized encoding: \"{format}\"").len();
    for (name, input, decoded) in [("decode", "ff", 0), ("encode", "\\xff", 1)] {
        let args = [Value::String(input.into()), Value::String(format.into())];
        let controls = controls(decoded + error_length - 1);
        // Act
        let result = evaluate(name, &args, Some(&controls))
            .unwrap()
            .map_err(CassieError::from);
        // Assert
        assert!(
            matches!(result, Err(CassieError::ResourceLimit(_))),
            "{name}: {result:?}"
        );
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_admit_invalid_format_error_given_exact_budget_when_codec_is_reached() {
    // Arrange
    let format = "bogus";
    let error_length = format!("unrecognized encoding: \"{format}\"").len();
    for (name, input, decoded) in [("decode", "ff", 0), ("encode", "\\xff", 1)] {
        let args = [Value::String(input.into()), Value::String(format.into())];
        let controls = self::controls(decoded + error_length);
        // Act
        let error = evaluate(name, &args, Some(&controls))
            .unwrap()
            .map_err(CassieError::from);
        // Assert
        assert!(
            matches!(error, Err(CassieError::InvalidParameterValue(message))
            if message == format!("unrecognized encoding: \"{format}\""))
        );
        assert_eq!(controls.peak_query_memory_bytes(), decoded + error_length);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
