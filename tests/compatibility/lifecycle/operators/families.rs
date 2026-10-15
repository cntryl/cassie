use super::*;

#[test]
fn should_publish_operator_family_members_given_an_added_member_when_the_transaction_commits() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("operator_family_member_commit").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE OPERATOR FAMILY compat_commit_family USING btree")
            .await
            .expect("create an empty btree operator family");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for the family member");
        client
            .batch_execute(
                "ALTER OPERATOR FAMILY compat_commit_family USING btree \
                 ADD OPERATOR 1 < (integer, integer)",
            )
            .await
            .expect("add integer less-than as strategy one");

        // Act
        client
            .batch_execute("COMMIT")
            .await
            .expect("commit the operator family member");
        let member = client
            .query_one(
                "SELECT count(*)::BIGINT FROM pg_catalog.pg_amop AS member \
                 JOIN pg_catalog.pg_opfamily AS family \
                   ON family.oid = member.amopfamily \
                 WHERE family.opfname = 'compat_commit_family' \
                   AND member.amopstrategy = 1 \
                   AND member.amoplefttype = 'integer'::regtype \
                   AND member.amoprighttype = 'integer'::regtype",
                &[],
            )
            .await
            .expect("read the committed operator-family member")
            .get::<_, i64>(0);

        // Assert
        assert_eq!(member, 1);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_operator_family_members_given_member_ddl_when_the_transaction_rolls_back() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("operator_family_member_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE OPERATOR FAMILY compat_rollback_family USING btree")
            .await
            .expect("create an empty btree operator family");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for the family member");
        client
            .batch_execute(
                "ALTER OPERATOR FAMILY compat_rollback_family USING btree \
                 ADD OPERATOR 1 < (integer, integer)",
            )
            .await
            .expect("add integer less-than as strategy one");

        // Act
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the operator family member");
        let member = client
            .query_one(
                "SELECT count(*)::BIGINT FROM pg_catalog.pg_amop AS member \
                 JOIN pg_catalog.pg_opfamily AS family \
                   ON family.oid = member.amopfamily \
                 WHERE family.opfname = 'compat_rollback_family'",
                &[],
            )
            .await
            .expect("read the operator-family catalog after rollback")
            .get::<_, i64>(0);

        // Assert
        assert_eq!(member, 0);
        drop(client);
        server.shutdown(connection).await;
    });
}
