use super::parse_text_payload;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use std::time::Instant;

#[test]
fn should_preserve_text_copy_fields_while_holding_decoder_admission() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let payload = b"\\N\ta\\tb\r\n\\\\N\tlast";
    // Act
    let (rows, memory) = parse_text_payload(payload, '\t', "\\N", &controls).expect("parse");
    // Assert
    assert_eq!(
        rows,
        vec![
            vec![None, Some("a\tb".into())],
            vec![Some("\\N".into()), Some("last".into())]
        ]
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_text_copy_row_directory_before_allocating_over_budget() {
    // Arrange
    let limits = CassieRuntimeLimits {
        query_memory_budget_bytes: 1,
        ..CassieRuntimeLimits::default()
    };
    let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
    // Act
    let result = parse_text_payload(b"one\ttwo\n", '\t', "\\N", &controls);
    // Assert
    assert!(result.is_err());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_preserve_text_copy_escape_and_final_record_behavior() {
    // Arrange
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let payload = b"\\101\t\\x41\tend\\";
    // Act
    let (rows, memory) = parse_text_payload(payload, '\t', "\\N", &controls).expect("parse");
    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Some("101".into()),
            Some("x41".into()),
            Some("end\\".into())
        ]]
    );
    drop((rows, memory));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
