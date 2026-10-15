#[test]
fn should_preserve_csv_null_marker_for_force_not_null_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_copy_force_not_null").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_issue846_copy_force_not_null \
                 (id INT, ordinary TEXT, forced TEXT)",
            )
            .await
            .expect("create a CSV COPY target with one forced text column");

        // Verify the standard COPY FROM STDIN path before the focused option case.
        let control = copy_in_frames(
            &server,
            "COPY compat_issue846_copy_force_not_null FROM STDIN WITH (FORMAT CSV)",
            b"0,baseline,baseline\n",
        )
        .await;
        assert_eq!(
            control.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "standard CSV COPY control enters COPY IN: {control:?}"
        );
        assert_eq!(support_pgwire::error_code(&control), None);
        assert_eq!(copy_command(&control).as_deref(), Some("COPY 1"));

        // Act
        let frames = copy_in_frames(
            &server,
            "COPY compat_issue846_copy_force_not_null (id, ordinary, forced) FROM STDIN \
             WITH (FORMAT CSV, FORCE_NOT_NULL (forced))",
            b"1,,\n2,plain,plain\n",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let command = copy_command(&frames);
        let rows = client
            .query(
                "SELECT id, ordinary, forced \
                 FROM compat_issue846_copy_force_not_null ORDER BY id",
                &[],
            )
            .await
            .expect("read values imported with FORCE_NOT_NULL");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            frames.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "PostgreSQL 18.6 accepts FORCE_NOT_NULL and enters COPY IN: {frames:?}"
        );
        assert_eq!(error_code, None, "FORCE_NOT_NULL SQLSTATE: {frames:?}");
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(
            rows.iter()
                .map(|row| (
                    row.get::<_, i32>(0),
                    row.get::<_, Option<String>>(1),
                    row.get::<_, Option<String>>(2),
                ))
                .collect::<Vec<_>>(),
            vec![
                (0, Some("baseline".to_owned()), Some("baseline".to_owned())),
                (1, None, Some(String::new())),
                (2, Some("plain".to_owned()), Some("plain".to_owned())),
            ]
        );
    });
}
#[test]
fn should_apply_copy_default_marker_without_changing_csv_nulls() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_846_copy_default_marker").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_issue846_copy_default_marker \
                 (id INT, payload TEXT DEFAULT 'fresh')",
            )
            .await
            .expect("create a COPY target with a text default");

        // Verify the standard COPY FROM STDIN path before the focused option case.
        let control = copy_in_frames(
            &server,
            "COPY compat_issue846_copy_default_marker FROM STDIN WITH (FORMAT CSV)",
            b"0,baseline\n",
        )
        .await;
        assert_eq!(
            control.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "standard CSV COPY control enters COPY IN: {control:?}"
        );
        assert_eq!(support_pgwire::error_code(&control), None);
        assert_eq!(copy_command(&control).as_deref(), Some("COPY 1"));

        // Act
        let frames = copy_in_frames(
            &server,
            "COPY compat_issue846_copy_default_marker (id, payload) FROM STDIN \
             WITH (FORMAT CSV, DEFAULT 'DEFAULT')",
            b"1,DEFAULT\n2,\n3,actual\n",
        )
        .await;
        let error_code = support_pgwire::error_code(&frames);
        let command = copy_command(&frames);
        let rows = client
            .query(
                "SELECT id, payload FROM compat_issue846_copy_default_marker ORDER BY id",
                &[],
            )
            .await
            .expect("read imported DEFAULT marker and null values");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(
            frames.first().map(|(tag, _)| *tag),
            Some(b'G'),
            "PostgreSQL 18.6 accepts COPY DEFAULT marker and enters COPY IN: {frames:?}"
        );
        assert_eq!(error_code, None, "COPY DEFAULT SQLSTATE: {frames:?}");
        assert_eq!(command.as_deref(), Some("COPY 3"));
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, Option<String>>(1),))
                .collect::<Vec<_>>(),
            vec![
                (0, Some("baseline".to_owned())),
                (1, Some("fresh".to_owned())),
                (2, None),
                (3, Some("actual".to_owned())),
            ]
        );
    });
}
