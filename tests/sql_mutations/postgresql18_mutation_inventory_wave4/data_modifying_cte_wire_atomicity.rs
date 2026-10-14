//! PostgreSQL 18 data-modifying CTE error and rollback behavior for #834.

use super::{current_thread_runtime, db_error, PgwireFixture};

#[test]
fn should_roll_back_the_delete_cte_on_outer_unique_violation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let fixture = PgwireFixture::start("issue-834-cte-wire-atomicity").await;
        for statement in [
            "CREATE TABLE issue_834_wire_atomic_source (id INT PRIMARY KEY)",
            "CREATE TABLE issue_834_wire_atomic_target (id INT PRIMARY KEY)",
            "INSERT INTO issue_834_wire_atomic_source VALUES (1)",
            "INSERT INTO issue_834_wire_atomic_target VALUES (1)",
        ] {
            if let Err(error) = fixture.client().batch_execute(statement).await {
                fixture.shutdown().await;
                panic!("setup statement failed: {statement}: {error:?}");
            }
        }

        // Act
        let rejected = fixture
            .client()
            .execute(
                "WITH removed AS ( \
                     DELETE FROM issue_834_wire_atomic_source WHERE id = 1 RETURNING id \
                 ) \
                 INSERT INTO issue_834_wire_atomic_target SELECT id FROM removed",
                &[],
            )
            .await;
        let sqlstate = rejected
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let counts = fixture
            .client()
            .query_one(
                "SELECT \
                   (SELECT count(*) FROM issue_834_wire_atomic_source), \
                   (SELECT count(*) FROM issue_834_wire_atomic_target)",
                &[],
            )
            .await
            .map(|row| (row.get::<_, i64>(0), row.get::<_, i64>(1)));
        fixture.shutdown().await;

        // Assert
        assert_eq!(
            sqlstate.as_deref(),
            Some("23505"),
            "the outer INSERT must report its unique violation: {rejected:?}"
        );
        assert_eq!(
            counts.expect("read rows after failed outer INSERT"),
            (1, 1),
            "the failed statement must roll back the DELETE CTE and preserve the target"
        );
    });
}
