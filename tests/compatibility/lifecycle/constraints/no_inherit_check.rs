use super::*;

#[test]
fn should_keep_no_inherit_check_on_parent_without_enforcing_it_on_child() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("no_inherit_check_constraint").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_no_inherit_parent (value INT, \
                   CONSTRAINT compat_no_inherit_check CHECK (value >= 0) NO INHERIT)",
                "CREATE TABLE compat_no_inherit_child () \
                   INHERITS (compat_no_inherit_parent)",
                "INSERT INTO compat_no_inherit_child VALUES (-1)",
            ],
        )
        .await
        .expect("create the parent constraint and accept a violating child row");
        let parent_constraint = client
            .query_one(
                "SELECT connoinherit, convalidated, conenforced \
                 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_no_inherit_check' \
                   AND conrelid = 'compat_no_inherit_parent'::regclass",
                &[],
            )
            .await
            .expect("read the parent CHECK constraint metadata");
        let child_check_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conrelid = 'compat_no_inherit_child'::regclass \
                   AND contype = 'c'",
                &[],
            )
            .await
            .expect("count CHECK constraints inherited by the child")
            .try_get(0)
            .expect("child CHECK constraint count");

        // Act
        let rejected_parent_row = client
            .execute("INSERT INTO compat_no_inherit_parent VALUES (-2)", &[])
            .await
            .expect_err("the parent must continue enforcing its NO INHERIT check");
        let parent_rows = client
            .query("SELECT value FROM ONLY compat_no_inherit_parent", &[])
            .await
            .expect("read rows stored directly in the parent after rejection");
        let child_rows = client
            .query(
                "SELECT value FROM ONLY compat_no_inherit_child ORDER BY value",
                &[],
            )
            .await
            .expect("read the child row accepted without the parent check");

        // Assert
        assert!(parent_constraint.get::<_, bool>(0));
        assert!(parent_constraint.get::<_, bool>(1));
        assert!(parent_constraint.get::<_, bool>(2));
        assert_eq!(child_check_count, 0);
        assert_eq!(db_error(&rejected_parent_row).code().code(), "23514");
        assert!(parent_rows.is_empty());
        assert_eq!(child_rows.len(), 1);
        assert_eq!(child_rows[0].get::<_, i32>(0), -1);

        drop(client);
        server.shutdown(connection).await;
    });
}
