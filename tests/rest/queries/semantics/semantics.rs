#[test]
fn should_crud_collection_documents_through_rest() {
    // Arrange
    let path = data_dir("crud");
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    let collection = "rest_docs";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let body = serde_json::json!({
            "name": collection,
            "description": "REST collection fixture",
            "fields": [
                {"name": "title", "type": "text"},
                {"name": "payload", "type": "json"},
                {"name": "embedding", "type": "vector(2)"},
            ]
        });

        // Act
        let create =
            collections::create(&cassie, body.to_string().as_bytes()).expect("create collection");
        let list = collections::list(&cassie);
        let doc = documents::create(
            &cassie,
            collection,
            serde_json::json!({"title": "hello", "payload": {"k": 1}, "embedding": [1.0, 2.0]})
                .to_string()
                .as_bytes(),
        )
        .expect("create document");
        let doc_id = doc["id"].as_str().expect("id present");
        let got = documents::get(&cassie, collection, doc_id).expect("get document");
        let removed = documents::delete(&cassie, collection, doc_id).expect("delete document");

        // Assert
        assert_eq!(create["collection"], collection);
        assert!(list.contains(&canonical_relation_name("postgres", "public", collection)));
        assert_eq!(
            cassie
                .midge
                .collection_metadata(collection)
                .expect("collection metadata")
                .expect("stored collection metadata")
                .description
                .as_deref(),
            Some("REST collection fixture")
        );
        assert_eq!(
            cassie
                .catalog
                .get_collection(collection)
                .expect("catalog collection metadata")
                .description
                .as_deref(),
            Some("REST collection fixture")
        );
        assert_eq!(got["title"], "hello");
        assert_eq!(removed["deleted"], serde_json::Value::Bool(true));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_apply_declared_on_delete_actions_to_rest_document_deletes() {
    // Arrange
    let path = data_dir("rest_on_delete_actions");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    for sql in [
            "CREATE TABLE rest_action_parent (id INT PRIMARY KEY, name TEXT)",
            "CREATE TABLE rest_action_cascade (cid INT PRIMARY KEY, pid INT, CONSTRAINT rest_action_cascade_fk FOREIGN KEY (pid) REFERENCES rest_action_parent(id) ON DELETE CASCADE)",
            "CREATE TABLE rest_action_set_null (nid INT PRIMARY KEY, pid INT, CONSTRAINT rest_action_set_null_fk FOREIGN KEY (pid) REFERENCES rest_action_parent(id) ON DELETE SET NULL)",
            "CREATE TABLE rest_action_restrict (rid INT PRIMARY KEY, pid INT, CONSTRAINT rest_action_restrict_fk FOREIGN KEY (pid) REFERENCES rest_action_parent(id) ON DELETE RESTRICT)",
            "INSERT INTO rest_action_parent (id, name) VALUES (1, 'a'), (2, 'b')",
            "INSERT INTO rest_action_cascade (cid, pid) VALUES (10, 1)",
            "INSERT INTO rest_action_set_null (nid, pid) VALUES (20, 1)",
            "INSERT INTO rest_action_restrict (rid, pid) VALUES (30, 2)",
        ] {
            cassie.execute_sql(&session, sql, Vec::new()).expect("setup SQL");
        }
    let parent_row = |key: i64| {
        cassie
            .midge
            .scan_documents("postgres.public.rest_action_parent")
            .expect("scan parent")
            .into_iter()
            .find(|document| document.payload["id"] == key)
            .expect("parent row")
            .id
    };
    let cascading_parent = parent_row(1);
    let restricted_parent = parent_row(2);

    // Act
    let cascaded = documents::delete(&cassie, "rest_action_parent", &cascading_parent);
    let restricted = documents::delete(&cassie, "rest_action_parent", &restricted_parent);
    let rows = |sql: &str| {
        cassie
            .execute_sql(&session, sql, Vec::new())
            .expect("select")
            .rows()
            .to_vec()
    };

    // Assert
    assert!(cascaded.is_ok(), "no child restricts parent 1");
    assert!(restricted.is_err(), "ON DELETE RESTRICT keeps parent 2");
    assert_eq!(
        rows("SELECT cid FROM rest_action_cascade"),
        [] as [std::vec::Vec<cassie::types::Value>; 0]
    );
    assert_eq!(
        rows("SELECT nid, pid FROM rest_action_set_null"),
        vec![vec![
            cassie::types::Value::Int64(20),
            cassie::types::Value::Null
        ]]
    );
    assert_eq!(
        rows("SELECT id FROM rest_action_parent"),
        vec![vec![cassie::types::Value::Int64(2)]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_create_rest_collection_for_session_database_scope() {
    // Arrange
    let path = data_dir("rest_collection_session_scope");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let default_session = cassie.create_session("root", Some("postgres".to_string()));
    cassie
        .execute_sql(&default_session, "CREATE DATABASE analytics", Vec::new())
        .expect("create analytics database");
    let session = cassie.create_session("root", Some("analytics".to_string()));
    cassie
        .execute_sql(&session, "CREATE SCHEMA tenant", Vec::new())
        .expect("create tenant schema");
    session.set_search_path(vec!["tenant".to_string(), "public".to_string()]);
    let body = serde_json::json!({
        "name": "rest_made",
        "fields": [{"name": "x", "type": "int"}],
    });

    // Act
    let result = collections::create_for_session(&cassie, &session, body.to_string().as_bytes());

    // Assert
    assert!(result.is_ok(), "collection creation failed: {result:?}");
    assert!(cassie
        .midge
        .collection_metadata("analytics.tenant.rest_made")
        .expect("scoped collection lookup")
        .is_some());
    assert!(cassie
        .midge
        .collection_metadata("postgres.public.rest_made")
        .expect("default collection lookup")
        .is_none());
    let query = cassie
        .execute_sql(&session, "SELECT x FROM rest_made", Vec::new())
        .expect("query newly created collection from session");
    assert_eq!(query.columns()[0].name, "x");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_apply_default_values_for_rest_ingest() {
    // Arrange
    let path = format!("/tmp/cassie-rest-default-{}", Uuid::new_v4());
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    let collection = "rest_constraint_defaults";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_defaults (id INT PRIMARY KEY, status TEXT DEFAULT 'pending')",
                vec![],
            )

.unwrap();

        // Act
        let doc = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1}).to_string().as_bytes(),
        )
        .expect("create rest document");
        let id = doc["id"].as_str().expect("id present");
        let stored = cassie
            .midge
            .get_document(collection, id)

            .expect("document read");

        // Assert
        let stored = stored.expect("document should be stored").payload;
        assert_eq!(stored["status"], "pending");
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_allow_case_distinct_rest_collection_fields() {
    // Arrange
    let path = data_dir("collection-case-distinct-fields");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    let body = serde_json::json!({
        "name": "rest_case_distinct_fields",
        "fields": [
            {"name": "Title", "type": "text"},
            {"name": "title", "type": "text"}
        ]
    });

    // Act
    let created = collections::create(&cassie, body.to_string().as_bytes());
    let insert = cassie.execute_sql(
        &session,
        "INSERT INTO rest_case_distinct_fields (\"Title\", \"title\") VALUES ('upper', 'lower')",
        Vec::new(),
    );
    let selected = cassie.execute_sql(
        &session,
        "SELECT \"Title\", \"title\" FROM rest_case_distinct_fields",
        Vec::new(),
    );

    // Assert
    assert!(
        created.is_ok(),
        "REST collection creation failed: {created:?}"
    );
    assert!(
        insert.is_ok(),
        "SQL insert should accept both case-distinct fields"
    );
    let selected = selected.expect("select case-distinct fields");
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["Title", "title"]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            cassie::types::Value::String("upper".to_string()),
            cassie::types::Value::String("lower".to_string())
        ]]
    );

    let _ = std::fs::remove_dir_all(path);
}
