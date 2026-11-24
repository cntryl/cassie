//! Deferred #809 PostgreSQL 18.6 nested TEXT[] key-existence witnesses.
use super::{fixture, wire};

#[test]
fn should_visit_all_nested_text_array_keys_for_jsonb_existence() {
    // Arrange
    let sql = r#"SELECT '{"a":1,"b":2}'::JSONB ?| '{{z,b}}', '{"a":1,"b":2}'::JSONB ?& '{{a,z}}', '{"a":1,"b":2}'::JSONB ?& '{{a,b}}', '{"a":1,"b":2}'::JSONB ?| '[0:0][3:4]={{z,b}}', '{"a":1,"b":2}'::JSONB ?& '[0:0][3:4]={{a,z}}', '{"a":1}'::JSONB ?| '{{z,NULL}}', '{"a":1}'::JSONB ?& '{{a,NULL}}'"#;
    // Act
    let batches = fixture::run_cycles(
        "json-nested-text-array-existence",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("t".into()),
            Some("f".into()),
            Some("t".into()),
            Some("t".into()),
            Some("f".into()),
            Some("f".into()),
            Some("t".into())
        ]]
    );
}

#[test]
fn should_reject_ragged_multidimensional_key_arrays() {
    // Arrange
    let sql = r#"SELECT '{"a":1}'::JSONB ?| '{{a},{b,c}}'"#;
    // Act
    let batches = fixture::run_cycles(
        "json-ragged-text-array-existence",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
}
