//! Index runtime failures retain their SQLSTATE and transaction boundary.
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use crate::support_type_metadata_contract::Frames;

fn index_error(label: &str, index: &str) -> Vec<Frames> {
    let statements = [
        "CREATE TABLE validation_errors (id INT4 PRIMARY KEY, code TEXT)",
        index,
        "BEGIN",
        "INSERT INTO validation_errors VALUES (0,NULL)",
        "ROLLBACK",
        "SELECT count(*) FROM validation_errors",
    ];
    run_script(
        label,
        statements
            .into_iter()
            .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
            .collect(),
    )
}

fn assert_index_error(frames: &[Frames]) {
    for batch in &frames[..3] {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(wire::error_code(&frames[3]).as_deref(), Some("22012"));
    assert!(!frames[3].iter().any(|(tag, _)| matches!(tag, b'D' | b'C')));
    assert_eq!(
        frames[3].last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
    assert_eq!(wire::error_code(&frames[4]), None);
    assert_eq!(
        frames[4].last().map(|frame| frame.1.as_slice()),
        Some(&b"I"[..])
    );
    assert_eq!(wire::error_code(&frames[5]), None);
    assert_eq!(
        wire::data_rows(&frames[5]),
        vec![vec![Some("0".to_owned())]]
    );
}

#[test]
fn should_preserve_predicate_error_given_null_index_key_when_transaction_is_active() {
    // Arrange
    let index = "CREATE UNIQUE INDEX validation_predicate ON validation_errors(code) WHERE 1/id>0";
    // Act
    let frames = index_error("issue952-predicate-state", index);
    // Assert
    assert_index_error(&frames);
}

#[test]
fn should_preserve_expression_error_given_valid_definition_when_transaction_is_active() {
    // Arrange
    let index = "CREATE UNIQUE INDEX validation_expression ON validation_errors((1/id))";
    // Act
    let frames = index_error("issue952-expression-state", index);
    // Assert
    assert_index_error(&frames);
}
