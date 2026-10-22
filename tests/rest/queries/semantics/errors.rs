#[test]
fn should_reject_rest_delete_of_referenced_parent_row() {
    // Arrange
    let path = data_dir("rest_referenced_parent_delete");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    for sql in [
        "CREATE TABLE rest_fk_parent (pid INT PRIMARY KEY)",
        "CREATE TABLE rest_fk_child (cid INT PRIMARY KEY, pid INT REFERENCES rest_fk_parent(pid))",
        "INSERT INTO rest_fk_parent VALUES (1)",
        "INSERT INTO rest_fk_child VALUES (10, 1)",
    ] {
        cassie
            .execute_sql(&session, sql, Vec::new())
            .expect("setup SQL");
    }
    let parent_id = cassie
        .midge
        .scan_documents("postgres.public.rest_fk_parent")
        .expect("scan parent")
        .into_iter()
        .next()
        .expect("parent row")
        .id;

    // Act
    let result = documents::delete(&cassie, "rest_fk_parent", &parent_id);

    // Assert
    let error = result.expect_err("REST must reject a referenced parent deletion");
    assert!(
        error.to_string().contains("still references"),
        "expected a referential restriction, got {error}"
    );
    let parent = cassie
        .midge
        .get_document("postgres.public.rest_fk_parent", &parent_id)
        .expect("read parent")
        .expect("restricted parent survives");
    assert_eq!(parent.payload["pid"], 1);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_invalid_vector_dimensions_through_rest() {
    // Arrange
    let path = data_dir("bad_vector");
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    let collection = "rest_bad_vector";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let _ = collections::create(
            &cassie,
            serde_json::json!({
                "name": collection,
                "fields": [{"name": "embedding", "type": "vector(2)"}],
            })
            .to_string()
            .as_bytes(),
        );

        // Act
        let insert = documents::create(
            &cassie,
            collection,
            serde_json::json!({"embedding": [1.0, 2.0, 3.0]})
                .to_string()
                .as_bytes(),
        );

        // Assert
        assert!(insert.is_err(), "dimension mismatch should fail");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_missing_document_lookup_through_rest() {
    // Arrange
    let path = data_dir("missing_doc");
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    let collection = "rest_missing_doc";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let _ = collections::create(
            &cassie,
            serde_json::json!({
                "name": collection,
                "fields": [{"name": "title", "type": "text"}],
            })
            .to_string()
            .as_bytes(),
        );

        // Act
        let missing = documents::get(&cassie, collection, "missing-id");
        // Assert
        assert!(missing.is_err(), "missing document should fail");
        let error = missing.unwrap_err().to_string();
        assert!(
            error.contains("document not found"),
            "unexpected error: {error}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_rest_ingest_when_not_null_constraint_is_violated() {
    // Arrange
    let path = format!("/tmp/cassie-rest-not-null-{}", Uuid::new_v4());
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    let collection = "rest_constraint_not_null";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_not_null (id INT PRIMARY KEY, email TEXT NOT NULL)",
                vec![],
            )
            .unwrap();

        // Act
        let missing = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1}).to_string().as_bytes(),
        );

        // Assert
        assert!(
            missing.is_err(),
            "missing required field should be rejected"
        );
        let error = missing.unwrap_err().to_string();
        assert!(
            error.contains("cannot be null"),
            "unexpected error: {error}"
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_rest_ingest_when_unique_constraint_is_violated() {
    // Arrange
    let path = format!("/tmp/cassie-rest-unique-{}", Uuid::new_v4());
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    let collection = "rest_constraint_unique";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_unique (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE)",
                vec![],
            )

.unwrap();

        documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1, "email": "a@example.com"})
                .to_string()
                .as_bytes(),
        )
        .expect("first insert");

        // Act
        let duplicate = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 2, "email": "a@example.com"})
                .to_string()
                .as_bytes(),
        );

        // Assert
        assert!(duplicate.is_err(), "duplicate unique field should be rejected");
        let error = duplicate.unwrap_err().to_string();
        assert!(
            error.contains("unique constraint failed"),
            "unexpected error: {error}"
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_rest_ingest_when_check_constraint_is_violated() {
    // Arrange
    let path = format!("/tmp/cassie-rest-check-{}", Uuid::new_v4());
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    let collection = "rest_constraint_check";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_constraint_check (id INT PRIMARY KEY, score INT CHECK (score >= 18))",
                vec![],
            )

.unwrap();

        // Act
        let invalid = documents::create(
            &cassie,
            collection,
            serde_json::json!({"id": 1, "score": 17})
                .to_string()
                .as_bytes(),
        );

        // Assert
        assert!(invalid.is_err(), "check constraint failure should be rejected");
        let error = invalid.unwrap_err().to_string();
        assert!(
            error.contains("check constraint failed"),
            "unexpected error: {error}"
        );
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_rest_collection_creation_that_reuses_an_existing_table() {
    // Arrange
    let path = data_dir("collection-create-existing-table");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_collection_parent (parent_id INT PRIMARY KEY)",
            vec![],
        )
        .expect("create parent table");
    cassie
            .execute_sql(
                &session,
                "CREATE TABLE RestExisting (id INT PRIMARY KEY, parent_id INT NOT NULL REFERENCES rest_collection_parent(parent_id), score INT CHECK (score >= 18), status TEXT DEFAULT 'pending')",
                vec![],
            )
            .expect("create table");
    let collection = canonical_relation_name("postgres", "public", "RestExisting");
    let original_schema = serde_json::to_value(
        cassie
            .catalog
            .get_schema(&collection)
            .expect("original catalog schema"),
    )
    .expect("serialize original catalog schema");
    let original_storage_schema = serde_json::to_value(
        cassie
            .midge
            .collection_schema(&collection)
            .expect("original storage schema"),
    )
    .expect("serialize original storage schema");
    let original_constraints = cassie.catalog.get_constraints(&collection);

    // Act
    let exact_name = collections::create(
        &cassie,
        serde_json::json!({
            "name": "RestExisting",
            "fields": [{"name": "replacement", "type": "text"}]
        })
        .to_string()
        .as_bytes(),
    );
    let case_variant = collections::create(
        &cassie,
        serde_json::json!({
            "name": "restexisting",
            "fields": [{"name": "replacement", "type": "text"}]
        })
        .to_string()
        .as_bytes(),
    );

    // Assert
    assert_relation_already_exists(&exact_name);
    assert_relation_already_exists(&case_variant);
    assert_eq!(
        serde_json::to_value(
            cassie
                .catalog
                .get_schema(&collection)
                .expect("catalog schema after rejected requests"),
        )
        .expect("serialize catalog schema after rejected requests"),
        original_schema
    );
    assert_eq!(
        serde_json::to_value(
            cassie
                .midge
                .collection_schema(&collection)
                .expect("storage schema after rejected requests"),
        )
        .expect("serialize storage schema after rejected requests"),
        original_storage_schema
    );
    assert_eq!(
        cassie.catalog.get_constraints(&collection),
        original_constraints
    );
    assert_table_integrity_constraints(&original_constraints);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_rest_collection_creation_that_reuses_an_existing_view() {
    // Arrange
    let path = data_dir("collection-create-existing-view");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE rest_collection_view_source (id INT)",
            vec![],
        )
        .expect("create source table");
    cassie
        .execute_sql(
            &session,
            "CREATE VIEW rest_collection_view AS SELECT id FROM rest_collection_view_source",
            vec![],
        )
        .expect("create view");
    let body = serde_json::json!({
        "name": "rest_collection_view",
        "fields": [{"name": "id", "type": "int"}]
    });

    // Act
    let result = collections::create(&cassie, body.to_string().as_bytes());

    // Assert
    assert!(
        result.is_err(),
        "REST collection creation must not create a table behind a view"
    );
    assert_eq!(
        cassie.midge.list_collections(),
        vec![canonical_relation_name(
            "postgres",
            "public",
            "rest_collection_view_source"
        )]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_concurrent_rest_collection_creation_when_sql_table_claims_name() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    let path = data_dir("collection-create-sql-race");
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("cassie"));
    cassie.startup().expect("startup");
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::CollectionCreate),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let (rest_tx, rest_rx) = mpsc::channel();
    let rest_cassie = Arc::clone(&cassie);
    let rest_worker = std::thread::spawn(move || {
        let _worker_guard = schema_write_conflict_worker_guard();
        let result = collections::create(
            &rest_cassie,
            serde_json::json!({
                "name": "RestConcurrent",
                "fields": [{"name": "rest_value", "type": "text"}]
            })
            .to_string()
            .as_bytes(),
        );
        rest_tx.send(result).expect("report REST creation");
    });
    ready.wait();
    set_schema_write_commit_barriers(None, None, None);
    let (sql_tx, sql_rx) = mpsc::channel();
    let sql_cassie = Arc::clone(&cassie);
    let sql_worker = std::thread::spawn(move || {
        let session = sql_cassie.create_session("root", None);
        let result = sql_cassie.execute_sql(
            &session,
            "CREATE TABLE restconcurrent (sql_value INT PRIMARY KEY)",
            vec![],
        );
        sql_tx.send(result).expect("report SQL creation");
    });

    // Act
    let early_sql_result = sql_rx.recv_timeout(Duration::from_millis(100)).ok();
    let sql_blocked = early_sql_result.is_none();
    resume.wait();
    let rest_result = rest_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("REST creation should finish");
    let sql_result = early_sql_result.unwrap_or_else(|| {
        sql_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("SQL creation should finish")
    });
    rest_worker.join().expect("join REST worker");
    sql_worker.join().expect("join SQL worker");

    // Assert
    assert!(
        sql_blocked,
        "SQL creation must wait until the REST relation publication completes"
    );
    assert!(rest_result.is_ok(), "REST creation failed: {rest_result:?}");
    assert!(matches!(
        sql_result,
        Err(CassieError::CatalogObjectAlreadyExists {
            kind: CatalogObjectKind::Relation,
            ..
        })
    ));
    let collection = canonical_relation_name("postgres", "public", "RestConcurrent");
    let catalog_schema = cassie
        .catalog
        .get_schema(&collection)
        .expect("catalog schema");
    assert_eq!(catalog_schema.fields.len(), 1);
    assert_eq!(catalog_schema.fields[0].name, "rest_value");
    assert_eq!(
        cassie.catalog.get_constraints(&collection),
        [] as [cassie::catalog::FieldConstraint; 0]
    );
    let storage_schema = cassie
        .midge
        .collection_schema(&collection)
        .expect("storage schema");
    assert_eq!(storage_schema.fields.len(), 1);
    assert_eq!(storage_schema.fields[0].name, "rest_value");

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_concurrent_rest_collection_creation_when_sql_view_claims_name() {
    // Arrange
    let _test_guard = schema_write_conflict_test_guard();
    let path = data_dir("collection-create-view-race");
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("cassie"));
    cassie.startup().expect("startup");
    let setup_session = cassie.create_session("root", None);
    cassie
        .execute_sql(
            &setup_session,
            "CREATE TABLE rest_view_race_source (id INT)",
            vec![],
        )
        .expect("create view source");
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    set_schema_write_commit_barriers(
        Some(SchemaWritePausePoint::CollectionCreate),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let (rest_tx, rest_rx) = mpsc::channel();
    let rest_cassie = Arc::clone(&cassie);
    let rest_worker = std::thread::spawn(move || {
        let _worker_guard = schema_write_conflict_worker_guard();
        let result = collections::create(
            &rest_cassie,
            serde_json::json!({
                "name": "RestViewRace",
                "fields": [{"name": "rest_value", "type": "text"}]
            })
            .to_string()
            .as_bytes(),
        );
        rest_tx.send(result).expect("report REST creation");
    });
    ready.wait();
    set_schema_write_commit_barriers(None, None, None);
    let (view_tx, view_rx) = mpsc::channel();
    let view_cassie = Arc::clone(&cassie);
    let view_worker = std::thread::spawn(move || {
        let session = view_cassie.create_session("root", None);
        let result = view_cassie.execute_sql(
            &session,
            "CREATE VIEW restviewrace AS SELECT id FROM rest_view_race_source",
            vec![],
        );
        view_tx.send(result).expect("report view creation");
    });

    // Act
    let early_view_result = view_rx.recv_timeout(Duration::from_millis(100)).ok();
    let view_blocked = early_view_result.is_none();
    resume.wait();
    let rest_result = rest_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("REST creation should finish");
    let view_result = early_view_result.unwrap_or_else(|| {
        view_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("view creation should finish")
    });
    rest_worker.join().expect("join REST worker");
    view_worker.join().expect("join view worker");

    // Assert
    assert!(
        view_blocked,
        "view creation must wait until the REST relation publication completes"
    );
    assert!(rest_result.is_ok(), "REST creation failed: {rest_result:?}");
    assert!(view_result.is_err(), "view creation unexpectedly succeeded");
    let collection = canonical_relation_name("postgres", "public", "RestViewRace");
    assert!(cassie.catalog.get_view(&collection).is_none());
    assert!(cassie.midge.collection_schema(&collection).is_some());

    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
