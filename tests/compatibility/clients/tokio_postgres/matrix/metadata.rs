#[test]
fn should_read_catalog_metadata_after_connect() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("connect_metadata").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        let messages = tokio::time::timeout(
            Duration::from_secs(5),
            client.simple_query("SELECT version(), current_schema(), current_database()"),
        )
        .await
        .expect("metadata query should complete within the timeout")
        .expect("query metadata");

        // Assert
        let row = messages
            .into_iter()
            .find_map(|message| match message {
                SimpleQueryMessage::Row(row) => Some(row),
                _ => None,
            })
            .expect("metadata query should return a row");
        assert!(row
            .get(0)
            .is_some_and(|version| version.starts_with("PostgreSQL 16.0 compatible Cassie ")));
        assert_eq!(row.get(1), Some("public"));
        assert_eq!(row.get(2), Some("postgres"));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_missing_relation_metadata_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("missing_relation").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        // Act
        let error = client
            .query_one("SELECT * FROM compat_missing_relation", &[])
            .await
            .expect_err("missing relation should be rejected");

        // Assert
        let db_error = db_error(&error);
        assert_eq!(db_error.code().code(), "42P01");
        assert_eq!(db_error.table(), Some("compat_missing_relation"));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_unique_violation_metadata_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("constraint_metadata").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        client
            .batch_execute(
                "CREATE TABLE compat_constraint_metadata (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE)",
            )
            .await
            .expect("table creation should succeed");
        client
            .execute(
                "INSERT INTO compat_constraint_metadata (id, email) VALUES ($1, $2)",
                &[&1_i32, &"alpha@example.com"],
            )
            .await
            .expect("seed insert should succeed");

        // Act
        let duplicate = client
            .execute(
                "INSERT INTO compat_constraint_metadata (id, email) VALUES ($1, $2)",
                &[&2_i32, &"alpha@example.com"],
            )
            .await
            .expect_err("duplicate unique value should be rejected");

        // Assert
        let duplicate = db_error(&duplicate);
        assert_eq!(duplicate.code().code(), "23505");
        assert_eq!(duplicate.table(), Some("compat_constraint_metadata"));
        assert_eq!(duplicate.column(), Some("email"));
        assert_eq!(
            duplicate.constraint(),
            Some("compat_constraint_metadata_email_unique")
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_not_null_violation_metadata_with_tokio_postgres() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let server = CompatibilityServer::start("not_null_metadata").await;
        let (client, connection) = tokio::time::timeout(Duration::from_secs(5), server.connect())
            .await
            .expect("connect should complete within the timeout");

        client
            .batch_execute(
                "CREATE TABLE compat_not_null_metadata (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE)",
            )
            .await
            .expect("table creation should succeed");

        // Act
        let missing_not_null = client
            .execute(
                "INSERT INTO compat_not_null_metadata (id, email) VALUES ($1, $2)",
                &[&3_i32, &Option::<String>::None],
            )
            .await
            .expect_err("null email should be rejected");

        // Assert
        let missing_not_null = db_error(&missing_not_null);
        assert_eq!(missing_not_null.code().code(), "23502");
        assert_eq!(missing_not_null.table(), Some("compat_not_null_metadata"));
        assert_eq!(missing_not_null.column(), Some("email"));
        assert_eq!(
            missing_not_null.constraint(),
            Some("compat_not_null_metadata_email_not_null")
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
