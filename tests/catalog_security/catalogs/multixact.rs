//! Row-level multitransaction member reporting (#817).

use crate::support_catalog_sessions as fixture;

#[test]
fn should_report_multixact_members_given_shared_row_locks_when_members_are_queried() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let fixture = fixture::start("issue-817-multixact-members").await;
        let first = fixture::connect(&fixture).await;
        let second = fixture::connect(&fixture).await;
        let observer = fixture::connect(&fixture).await;
        first
            .client
            .batch_execute(
                "CREATE TABLE issue_817_multixact_row (id INTEGER PRIMARY KEY, value INTEGER); \
                 INSERT INTO issue_817_multixact_row VALUES (1, 10)",
            )
            .await
            .expect("create a row for shared key locks");
        let mut expected_xids = Vec::with_capacity(2);
        for session in [&first, &second] {
            session
                .client
                .batch_execute("BEGIN")
                .await
                .expect("begin a shared-lock transaction");
            let locked = session
                .client
                .query_one(
                    "SELECT value FROM issue_817_multixact_row \
                     WHERE id = 1 FOR KEY SHARE",
                    &[],
                )
                .await
                .expect("hold a compatible key-share row lock");
            assert_eq!(locked.get::<_, i32>(0), 10);
            expected_xids.push(
                session
                    .client
                    .query_one("SELECT pg_current_xact_id()::text", &[])
                    .await
                    .expect("read the row-locking transaction ID")
                    .get::<_, String>(0),
            );
        }
        expected_xids.sort();

        // Act
        let rows = observer
            .client
            .query(
                "SELECT member.xid::text, member.mode::text \
                 FROM issue_817_multixact_row AS target \
                 CROSS JOIN LATERAL pg_catalog.pg_get_multixact_members(target.xmax) AS member \
                 WHERE target.id = 1 ORDER BY member.xid::text",
                &[],
            )
            .await
            .expect("read the members of the row's multitransaction");
        let members = rows
            .iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, String>(1)))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(
            members,
            expected_xids
                .into_iter()
                .map(|xid| (xid, "keysh".to_owned()))
                .collect::<Vec<_>>()
        );
        fixture::stop(fixture, vec![first, second, observer]).await;
    });
}
