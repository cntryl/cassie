//! Deferred #809 user-routine witness for nested literal extraction paths.
use super::{fixture, wire};

#[test]
fn should_follow_nested_text_array_literal_in_user_function_body() {
    // Arrange
    let setup = [(
        r#"CREATE FUNCTION nested_json_path() RETURNS TEXT AS "jsonb_build_object('a', jsonb_build_object('b', 7)) #>> '{{a,b}}'""#,
        Vec::new(),
    )];
    let cycle = fixture::execute_cycle("SELECT nested_json_path()", None, 0, true);
    // Act
    let batches = fixture::run_cycles("json-nested-user-function-path", &setup, vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![Some("7".into())]]);
}
