#[test]
fn should_persist_table_column_comments_in_catalogs_after_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("object-comments-restart");
    let _directory = DataDirectory(path.clone());
    let expected = vec![vec![
        cassie::types::Value::String("table documentation".into()),
        cassie::types::Value::String("column documentation".into()),
    ]];
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie instance");
        cassie.startup().expect("start Cassie instance");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE compat_comment_rows (id INT, label TEXT)",
            "COMMENT ON TABLE compat_comment_rows IS 'table documentation'",
            "COMMENT ON COLUMN compat_comment_rows.label IS 'column documentation'",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("could not execute {sql}: {error}"));
        }
        let comments = cassie
            .execute_sql(
                &session,
                "SELECT obj_description(c.oid, 'pg_class'), \
                 col_description(c.oid, a.attnum) \
                 FROM pg_catalog.pg_class AS c \
                 JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid \
                 WHERE c.relname = 'compat_comment_rows' \
                   AND a.attname = 'label' AND a.attnum > 0",
                vec![],
            )
            .expect("read comments from catalogs")
            .rows()
            .to_vec();
        assert_eq!(comments, expected);
        drop(session);
        drop(cassie);
    }

    // Act
    let cassie = Cassie::new_with_data_dir(&path).expect("reopen Cassie instance");
    cassie.startup().expect("recover persisted comments");
    let session = cassie.create_session("tester", None);
    let comments = cassie
        .execute_sql(
            &session,
            "SELECT obj_description(c.oid, 'pg_class'), \
             col_description(c.oid, a.attnum) \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_attribute AS a ON a.attrelid = c.oid \
             WHERE c.relname = 'compat_comment_rows' \
               AND a.attname = 'label' AND a.attnum > 0",
            vec![],
        )
        .expect("read comments after restart")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(comments, expected);
}
