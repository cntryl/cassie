#[test]
fn should_merge_jsonb_objects_with_right_hand_key_precedence() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_merge_values (left_document JSONB, right_document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_merge_values VALUES ('{\"a\":1,\"b\":0}', '{\"b\":2,\"c\":3}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT left_document || right_document FROM issue_809_jsonb_merge_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-concatenate-objects", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (114, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("{\"a\": 1, \"b\": 2, \"c\": 3}".to_owned())]]
    );
}

#[test]
fn should_preserve_jsonb_numbers_during_concatenation() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"n\":9007199254740993,\"scale\":1.00,\"exponent\":1e2}'::JSONB \
         || '{\"tail\":null}'::JSONB)::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-concat-exact-numbers", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(
            "{\"n\": 9007199254740993, \"tail\": null, \"scale\": 1.00, \"exponent\": 100}"
                .to_owned()
        )]]
    );
}

#[test]
fn should_format_nested_jsonb_concat_with_duplicate_keys() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"a\":{\"x\":9007199254740993,\"scale\":1.00,\"exponent\":1e2},\"dup\":0,\"dup\":1}'::JSONB \
         || '{\"b\":[1.00,9007199254740993],\"dup\":2,\"dup\":3}'::JSONB)::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-concat-nested-duplicates", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some(
            "{\"a\": {\"x\": 9007199254740993, \"scale\": 1.00, \"exponent\": 100}, \"b\": [1.00, 9007199254740993], \"dup\": 3}"
                .to_owned()
        )]]
    );
}

#[test]
fn should_replace_nested_object_values_on_jsonb_concatenation() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT ('{\"a\":{\"x\":1}}'::JSONB || '{\"a\":{\"y\":2}}'::JSONB)::TEXT",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-concat-shallow-merge", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("{\"a\": {\"y\": 2}}".to_owned())]]
    );
}
