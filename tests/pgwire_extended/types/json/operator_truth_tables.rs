//! JSONB operator truth tables for array and object key existence.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[test]
fn should_scope_jsonb_key_existence_to_top_level_values() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_key_scope (id INT, document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_key_scope VALUES (1, '[\"foo\", {\"foo\": 1}, null]'), (2, '{\"nested\":{\"foo\":1}}'), (3, NULL), (4, 'null'), (5, '{\"foo\":null}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT id, document ? 'foo', document ? 'null', document ? NULL::TEXT FROM issue_809_jsonb_key_scope ORDER BY id",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-key-existence-scope", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB key existence RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 16, 16, 16]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some("t".to_owned()),
                Some("f".to_owned()),
                None
            ],
            vec![
                Some("2".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned()),
                None
            ],
            vec![Some("3".to_owned()), None, None, None],
            vec![
                Some("4".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned()),
                None
            ],
            vec![
                Some("5".to_owned()),
                Some("t".to_owned()),
                Some("f".to_owned()),
                None
            ],
        ]
    );
}

#[test]
fn should_apply_json_exists_error_policies_to_strict_structural_errors() {
    // Arrange
    let policies = fixture::execute_cycle(
        "SELECT JSON_EXISTS('{\"a\":1}'::JSONB, 'strict $.a[*]' FALSE ON ERROR), \
         JSON_EXISTS('{\"a\":1}'::JSONB, 'strict $.a[*]' UNKNOWN ON ERROR), \
         JSON_EXISTS('{\"a\":1}'::JSONB, 'strict $.a[*]' TRUE ON ERROR)",
        None,
        0,
        true,
    );
    let error = fixture::execute_cycle(
        "SELECT JSON_EXISTS('{\"a\":1}'::JSONB, 'strict $.a[*]' ERROR ON ERROR)",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-json-exists-error-policies",
        &[],
        vec![policies, error, recovery],
    );

    // Assert
    let policy_result = &batches[0];
    fixture::assert_success(policy_result);
    let description = policy_result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_EXISTS policy RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16, 16]
    );
    assert_eq!(
        wire::data_rows(policy_result),
        vec![vec![Some("f".to_owned()), None, Some("t".to_owned())]]
    );
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("22039"));
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_construct_a_json_array_from_an_ordered_query_expression() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT JSON_ARRAY(SELECT item FROM (VALUES (5), (2), (NULL::INTEGER)) AS entries(item) ORDER BY item NULLS LAST)",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-json-array-query-expression", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (114, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[2, 5]".to_owned())]]
    );
}

#[test]
fn should_apply_unique_key_policy_to_json_constructor_inputs() {
    // Arrange
    let without_unique = fixture::execute_cycle(
        "SELECT JSON('{\"a\":1,\"a\":2}' WITHOUT UNIQUE KEYS)",
        None,
        0,
        true,
    );
    let with_unique = fixture::execute_cycle(
        "SELECT JSON('{\"a\":1,\"a\":2}' WITH UNIQUE KEYS)",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-json-constructor-unique-keys",
        &[],
        vec![without_unique, with_unique, recovery],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (114, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some(r#"{"a":1,"a":2}"#.to_owned())]]
    );

    let frames = &batches[1];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22030"));
    let errors = frames
        .iter()
        .filter(|(tag, _)| *tag == b'E')
        .collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    let message = wire::parse_error_fields(&errors[0].1)
        .into_iter()
        .find_map(|(key, value)| (key == 'M').then_some(value))
        .expect("duplicate JSON constructor key message");
    assert_eq!(message, "duplicate JSON object key value");
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());

    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}
