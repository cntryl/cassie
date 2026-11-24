use super::{current_thread_runtime, db_error, PgwireFixture};

#[test]
fn should_evaluate_first_candidate_returning_before_later_unique_conflict() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let fixture = PgwireFixture::start("issue-823-returning-error-order").await;
        for statement in [
            "CREATE TABLE issue_823_order_target (id BIGINT, unique_key BIGINT UNIQUE)",
            "CREATE TABLE issue_823_order_error (value BIGINT)",
            "BEGIN",
            "INSERT INTO issue_823_order_error VALUES (0)",
        ] {
            if let Err(error) = fixture.client().batch_execute(statement).await {
                fixture.shutdown().await;
                panic!("setup statement failed: {statement}: {error:?}");
            }
        }

        // Act
        let rejected = fixture
            .client()
            .query(
                "INSERT INTO issue_823_order_target VALUES (1, 7), (2, 7) \
                 RETURNING EXISTS (\
                   SELECT 1 FROM issue_823_order_error AS error_row \
                   WHERE error_row.value / 0 = 0\
                 ) AS selected",
                &[],
            )
            .await;
        let sqlstate = rejected
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        fixture
            .client()
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction after the RETURNING error");
        let target_rows = fixture
            .client()
            .query(
                "SELECT id, unique_key FROM issue_823_order_target ORDER BY id",
                &[],
            )
            .await
            .map(|rows| rows.len());

        // Assert
        fixture.shutdown().await;
        assert_eq!(
            sqlstate.as_deref(),
            Some("22012"),
            "PostgreSQL 18.6 evaluates the first candidate's row-dependent RETURNING expression \
             before processing the later unique conflict; got {rejected:?}"
        );
        assert_eq!(
            target_rows.expect("read target rows after rollback"),
            0,
            "a failed multi-row INSERT must publish no candidates"
        );
    });
}
