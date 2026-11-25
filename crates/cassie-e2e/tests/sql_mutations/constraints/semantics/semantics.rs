#[test]
fn should_keep_altered_primary_key_writable_given_a_differently_cased_column_name() {
    // Arrange
    let path = data_dir("alter_primary_key_column_case");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE primary_key_case (id INT, amount INT)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "ALTER TABLE primary_key_case ADD CONSTRAINT primary_key_case_pk PRIMARY KEY (ID)",
            vec![],
        )
        .expect("add primary key");

    // Act
    let first_insert = cassie.execute_sql(
        &session,
        "INSERT INTO primary_key_case (id, amount) VALUES (1, 5)",
        vec![],
    );
    let duplicate_insert = cassie.execute_sql(
        &session,
        "INSERT INTO primary_key_case (id, amount) VALUES (1, 6)",
        vec![],
    );

    // Assert
    assert!(
        first_insert.is_ok(),
        "case-mismatched PRIMARY KEY made the table unwritable"
    );
    assert!(
        matches!(
            duplicate_insert,
            Err(cassie::app::CassieError::UniqueViolation { .. })
        ),
        "case-mismatched PRIMARY KEY did not reject a duplicate"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_enforce_constraints_during_ingest() {
    // Arrange
    let path = data_dir("constraints_ingest");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let create = cassie
            .execute_sql(
                &session,
                "CREATE TABLE constraint_docs (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE, status TEXT DEFAULT 'pending', score INT CHECK (score >= 18))",
                vec![],
            )

.unwrap();
        let collection = canonical_test_collection(&cassie, "constraint_docs");

        let first = cassie
            .ingest_document(
                &collection,
                serde_json::json!({"id": 1, "email": "a@example.com", "score": 25}),
            )
            .unwrap();
        let missing_not_null = cassie
            .ingest_document(&collection, serde_json::json!({"id": 2, "score": 20}));
        let duplicate = cassie
            .ingest_document(
                &collection,
                serde_json::json!({"id": 3, "email": "a@example.com", "score": 19}),
            );
        let rejected_check = cassie
            .ingest_document(
                &collection,
                serde_json::json!({"id": 4, "email": "b@example.com", "score": 17}),
            );

        let inserted = cassie
            .midge
            .get_document(&collection, &first)

            .unwrap()
            .expect("document inserted");

        // Assert
        assert_eq!(create.command(), "CREATE TABLE");
        assert_eq!(
            inserted.payload.get("status").expect("status is defaulted"),
            &serde_json::Value::String("pending".to_string())
        );
        assert!(missing_not_null.is_err());
        assert!(missing_not_null
            .unwrap_err()
            .to_string()
            .contains("cannot be null"));
        assert!(duplicate.is_err());
        assert!(duplicate
            .unwrap_err()
            .to_string()
            .contains("unique constraint"));
        assert!(rejected_check.is_err());
        assert!(rejected_check
            .unwrap_err()
            .to_string()
            .contains("check constraint"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_hydrate_collection_constraints_on_startup() {
    // Arrange
    use_local_storage();
    let path = data_dir("constraints_hydrate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE hydrated_constraints (id INT, email TEXT NOT NULL UNIQUE, score INT CHECK (score >= 0))",
                vec![],
            )

.unwrap();

        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();

        let constraints = restarted.catalog.get_constraints("hydrated_constraints");
        // Assert
        assert_eq!(constraints.len(), 2);
        assert!(constraints.iter().any(|constraint| constraint.not_null));
        assert!(constraints.iter().any(|constraint| constraint.unique));
        assert!(constraints.iter().any(|constraint| constraint.check.is_some()));
    });

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_allow_only_one_concurrent_unique_insert_to_commit() {
    // Arrange
    let path = data_dir("unique_concurrent_insert");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_concurrent_insert (email TEXT UNIQUE)",
                vec![],
            )
            .unwrap();

        let cassie = std::sync::Arc::new(cassie);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let workers = (0..2)
            .map(|_| {
                let cassie = std::sync::Arc::clone(&cassie);
                let barrier = std::sync::Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let session = cassie.create_session("tester", None);
                    barrier.wait();
                    cassie.execute_sql(
                        &session,
                        "INSERT INTO unique_concurrent_insert (email) VALUES ('same@example.com')",
                        vec![],
                    )
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();

        // Act
        let results = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker completed"))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        assert!(results.iter().any(|result| {
            result
                .as_ref()
                .err()
                .is_some_and(|error| error.to_string().contains("unique constraint"))
        }));
        let selected = cassie
            .execute_sql(
                &cassie.create_session("tester", None),
                "SELECT email FROM unique_concurrent_insert",
                vec![],
            )
            .unwrap();
        assert_eq!(selected.rows().len(), 1);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_insert_on_conflict_do_update() {
    // Arrange
    let path = data_dir("on_conflict_do_update");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE on_conflict_do_update (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO on_conflict_do_update (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "INSERT INTO on_conflict_do_update (id, title) VALUES (1, 'beta') ON CONFLICT (id) DO UPDATE SET title = excluded.title RETURNING title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command(), "INSERT 0 1");
        assert_eq!(result.rows(), vec![vec![Value::String("beta".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_insert_on_conflict_do_nothing() {
    // Arrange
    let path = data_dir("on_conflict_do_nothing");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE on_conflict_do_nothing (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO on_conflict_do_nothing (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "INSERT INTO on_conflict_do_nothing (id, title) VALUES (1, 'beta') ON CONFLICT DO NOTHING",
                vec![],
            )
            .unwrap();
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT title FROM on_conflict_do_nothing ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.command(), "INSERT 0 0");
        assert_eq!(rows.rows(), vec![vec![Value::String("alpha".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}
