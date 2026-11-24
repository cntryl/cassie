#[test]
fn should_reject_truncate_restrict_before_changing_any_listed_table() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let fixture = PgwireFixture::start("issue-823-truncate-restrict-atomicity").await;
        let setup_statements = [
            "CREATE TABLE wave4_truncate_parent (id INT PRIMARY KEY)",
            "CREATE TABLE wave4_truncate_child (id INT PRIMARY KEY, parent_id INT \
               REFERENCES wave4_truncate_parent(id))",
            "CREATE TABLE wave4_truncate_unrelated (id INT PRIMARY KEY)",
            "INSERT INTO wave4_truncate_parent VALUES (1)",
            "INSERT INTO wave4_truncate_child VALUES (10, 1)",
            "INSERT INTO wave4_truncate_unrelated VALUES (99)",
        ];
        for sql in setup_statements {
            if let Err(error) = fixture.client().batch_execute(sql).await {
                fixture.shutdown().await;
                panic!("setup statement failed: {sql}: {error:?}");
            }
        }

        // Act
        let rejected = fixture
            .client()
            .batch_execute(
                "TRUNCATE TABLE wave4_truncate_unrelated, wave4_truncate_parent RESTRICT",
            )
            .await;
        let counts = fixture
            .client()
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM wave4_truncate_parent), \
                   (SELECT count(*) FROM wave4_truncate_child), \
                   (SELECT count(*) FROM wave4_truncate_unrelated)",
                &[],
            )
            .await;
        fixture.shutdown().await;

        // Assert
        let error = rejected.expect_err("RESTRICT must reject the referenced parent");
        let database_error = db_error(&error);
        assert_eq!(database_error.code().code(), "0A000");
        assert_eq!(
            database_error.message(),
            "cannot truncate a table referenced in a foreign key constraint"
        );
        assert!(database_error
            .detail()
            .expect("referencing table detail")
            .contains("wave4_truncate_child"));
        let counts = counts.expect("read rows after rejected TRUNCATE");
        assert_eq!(
            (
                counts.get::<_, i64>(0),
                counts.get::<_, i64>(1),
                counts.get::<_, i64>(2),
            ),
            (1, 1, 1),
            "RESTRICT rejection must not truncate another table from the same command"
        );
    });
}
