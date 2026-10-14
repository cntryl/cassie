//! PostgreSQL 18 MERGE cardinality and statement atomicity for #823.

use super::{current_thread_runtime, db_error, PgwireFixture};

#[test]
fn should_reject_multiple_source_matches_without_persisting_any_merge_action() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let fixture = PgwireFixture::start("issue-823-merge-cardinality-atomicity").await;
        for statement in [
            "CREATE TABLE issue_823_cardinality_target (id INT PRIMARY KEY, payload TEXT)",
            "CREATE TABLE issue_823_cardinality_source (id INT, payload TEXT)",
            "INSERT INTO issue_823_cardinality_target VALUES (1, 'before'), (2, 'unchanged')",
            "INSERT INTO issue_823_cardinality_source VALUES \
                (1, 'first'), (1, 'second'), (2, 'would-change')",
        ] {
            if let Err(error) = fixture.client().batch_execute(statement).await {
                fixture.shutdown().await;
                panic!("setup statement failed: {statement}: {error:?}");
            }
        }

        // Act
        let rejected = fixture
            .client()
            .batch_execute(
                "MERGE INTO issue_823_cardinality_target AS target \
                 USING issue_823_cardinality_source AS source ON target.id = source.id \
                 WHEN MATCHED THEN UPDATE SET payload = source.payload \
                 RETURNING target.id, target.payload",
            )
            .await;
        let sqlstate = rejected
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let persisted = fixture
            .client()
            .query(
                "SELECT id, payload FROM issue_823_cardinality_target ORDER BY id",
                &[],
            )
            .await
            .map(|rows| {
                rows.iter()
                    .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                    .collect::<Vec<_>>()
            });
        fixture.shutdown().await;

        // Assert
        assert_eq!(
            sqlstate.as_deref(),
            Some("21000"),
            "a target row matched by multiple MERGE source rows must raise cardinality_violation: {rejected:?}"
        );
        assert_eq!(
            persisted.expect("read target after failed MERGE"),
            vec![
                (1, "before".to_owned()),
                (2, "unchanged".to_owned()),
            ],
            "the failed MERGE must roll back every target action from the statement"
        );
    });
}
