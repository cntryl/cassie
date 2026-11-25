use super::{current_thread_runtime, PgwireFixture};

#[test]
fn should_preserve_update_cte_returning_row_description() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let fixture = PgwireFixture::start("issue-834-update-cte-row-description").await;
        for statement in [
            "CREATE TABLE issue_834_update_returning_wire \
               (id INT PRIMARY KEY, payload TEXT NOT NULL, optional INT)",
            "INSERT INTO issue_834_update_returning_wire VALUES (1, 'before', 7)",
        ] {
            if let Err(error) = fixture.client().batch_execute(statement).await {
                fixture.shutdown().await;
                panic!("setup statement failed: {statement}: {error:?}");
            }
        }
        let statement = match fixture
            .client()
            .prepare(
                "WITH changed AS ( \
                     UPDATE issue_834_update_returning_wire \
                     SET payload = 'after', optional = NULL WHERE id = 1 \
                     RETURNING id, payload, optional \
                 ) \
                 SELECT id, payload, optional FROM changed",
            )
            .await
        {
            Ok(statement) => statement,
            Err(error) => {
                fixture.shutdown().await;
                panic!("prepare UPDATE RETURNING CTE: {error:?}");
            }
        };

        // Act
        let returned = fixture.client().query_one(&statement, &[]).await;
        let stored = fixture
            .client()
            .query_one(
                "SELECT id, payload, optional FROM issue_834_update_returning_wire",
                &[],
            )
            .await;
        let column_oids = statement
            .columns()
            .iter()
            .map(|column| column.type_().oid())
            .collect::<Vec<_>>();
        fixture.shutdown().await;

        // Assert
        assert_eq!(column_oids, vec![23, 25, 23]);
        let returned = returned.expect("read UPDATE RETURNING rows through the CTE");
        assert_eq!(returned.get::<_, i32>(0), 1);
        assert_eq!(returned.get::<_, String>(1), "after");
        assert_eq!(returned.get::<_, Option<i32>>(2), None);
        let stored = stored.expect("read the persisted row after the CTE statement");
        assert_eq!(stored.get::<_, i32>(0), 1);
        assert_eq!(stored.get::<_, String>(1), "after");
        assert_eq!(stored.get::<_, Option<i32>>(2), None);
    });
}
