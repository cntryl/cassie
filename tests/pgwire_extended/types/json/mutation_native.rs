use super::{fixture, wire};

#[test]
fn should_preserve_jsonb_mutation_boundaries_given_missing_paths_and_array_indexes_when_documents_change(
) {
    // Arrange
    let sql = "SELECT jsonb_set('{}','{missing,NULL}','1'), jsonb_set('[]','{bad}','2',false), jsonb_insert('[1,2]','{-10}','9',true), jsonb_set('{\"a\":1}','{a,b}','2'), jsonb_set_lax('{}','{a}','1',true,'bad')";
    // Act
    let batches = fixture::run_cycles(
        "jsonb-native-mutation-boundaries",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("{}".into()),
            Some("[]".into()),
            Some("[9, 1, 2]".into()),
            Some(r#"{"a": 1}"#.into()),
            Some(r#"{"a": 1}"#.into())
        ]]
    );
}

#[test]
fn should_reject_jsonb_mutation_errors_given_invalid_paths_when_the_connection_is_reused() {
    // Arrange
    let cases = [
        ("SELECT jsonb_set('1','{}','2')", "22023"),
        ("SELECT jsonb_set('[]','{bad}','2')", "22P02"),
        ("SELECT jsonb_set('{}','{NULL}','2')", "22004"),
        ("SELECT jsonb_set_lax('{}','{a}','1',true,NULL)", "22023"),
        ("SELECT jsonb_insert('{\"a\":1}','{a}','2')", "22023"),
    ];
    let cycles = cases
        .iter()
        .flat_map(|(sql, _)| {
            [
                fixture::execute_cycle(sql, None, 0, true),
                fixture::execute_cycle("SELECT 7::INT", None, 0, true),
            ]
        })
        .collect();
    // Act
    let batches = fixture::run_cycles("jsonb-native-mutation-errors", &[], cycles);
    // Assert
    for ((_, code), frames) in cases.iter().zip(batches.as_chunks::<2>().0) {
        assert_eq!(wire::error_code(&frames[0]).as_deref(), Some(*code));
        assert_eq!(
            wire::data_rows(&frames[0]),
            Vec::<Vec<Option<String>>>::new()
        );
        fixture::assert_success(&frames[1]);
        assert_eq!(wire::data_rows(&frames[1]), vec![vec![Some("7".into())]]);
    }
}

#[test]
fn should_preserve_json_member_order_given_null_stripping_when_duplicate_nonnull_keys_remain() {
    // Arrange
    let source = r#"'{"z":null,"b":1,"b":2,"aa":{"x":null,"v":[null,1]}}'"#;
    let sql = format!("SELECT json_strip_nulls({source}::JSON,true), jsonb_strip_nulls({source}::JSONB,true), json_strip_nulls('null'::JSON), jsonb_strip_nulls(NULL::JSONB)");
    // Act
    let batches = fixture::run_cycles(
        "json-native-null-strip-order",
        &[],
        vec![fixture::execute_cycle(&sql, None, 0, true)],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some(r#"{"b":1,"b":2,"aa":{"v":[1]}}"#.into()),
            Some(r#"{"b": 2, "aa": {"v": [1]}}"#.into()),
            Some("null".into()),
            None
        ]]
    );
}

#[test]
fn should_preserve_exact_jsonb_pretty_output_given_wide_numbers_when_document_nulls_are_formatted()
{
    // Arrange
    let sql = "SELECT jsonb_pretty('{\"n\":123456789012345678901234567890.125}'::JSONB), jsonb_pretty('null'::JSONB), jsonb_pretty(NULL::JSONB)";
    // Act
    let batches = fixture::run_cycles(
        "jsonb-native-pretty-exact",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("{\n    \"n\": 123456789012345678901234567890.125\n}".into()),
            Some("null".into()),
            None
        ]]
    );
}
