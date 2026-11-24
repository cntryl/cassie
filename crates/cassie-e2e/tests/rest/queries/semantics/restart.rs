#[test]
fn should_reject_noncanonical_rest_collection_names_before_persisting_them() {
    // Arrange
    let path = data_dir("collection-create-invalid-name");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    let requests = ["", "   ", "other.public.events", "a b", "my.table", "x/y"];

    // Act
    let results = requests
        .iter()
        .map(|name| {
            collections::create(
                &cassie,
                serde_json::json!({
                    "name": name,
                    "fields": [{"name": "value", "type": "text"}]
                })
                .to_string()
                .as_bytes(),
            )
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        results.iter().all(Result::is_err),
        "REST collection creation accepted a noncanonical name: {results:?}"
    );
    assert!(
        cassie.midge.list_collections().is_empty(),
        "invalid collection names must not be persisted"
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_invalid_rest_collection_fields_before_persisting_them() {
    // Arrange
    let path = data_dir("collection-create-invalid-fields");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    let requests = [
        serde_json::json!({
            "name": "rest_duplicate_fields",
            "fields": [
                {"name": "Title", "type": "text"},
                {"name": "Title", "type": "int"}
            ]
        }),
        serde_json::json!({
            "name": "rest_empty_field",
            "fields": [{"name": "   ", "type": "text"}]
        }),
        serde_json::json!({
            "name": "rest_reserved_identity",
            "fields": [{"name": "_id", "type": "text"}]
        }),
        serde_json::json!({
            "name": "rest_reserved_identity_case",
            "fields": [{"name": "_ID", "type": "text"}]
        }),
        serde_json::json!({"name": "rest_empty_definition", "fields": []}),
        serde_json::json!({"name": "rest_missing_definition"}),
    ];

    // Act
    let results = requests
        .iter()
        .map(|request| collections::create(&cassie, request.to_string().as_bytes()))
        .collect::<Vec<_>>();

    // Assert
    assert!(
        results.iter().all(Result::is_err),
        "REST collection creation accepted an invalid field definition: {results:?}"
    );
    assert!(
        cassie.midge.list_collections().is_empty(),
        "a rejected collection definition must not be persisted"
    );

    let _ = std::fs::remove_dir_all(path);
}
