use super::*;

#[test]
fn should_reject_comment_writes_from_a_non_owner() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("comment_non_owner_update").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat845_comment_owner NOLOGIN",
                "CREATE ROLE compat845_comment_reader NOLOGIN",
                "CREATE TABLE compat845_comment_owned_rows (id INT PRIMARY KEY)",
                "ALTER TABLE compat845_comment_owned_rows \
                   OWNER TO compat845_comment_owner",
                "INSERT INTO compat845_comment_owned_rows VALUES (7)",
                "GRANT USAGE ON SCHEMA public TO compat845_comment_reader",
                "GRANT SELECT ON compat845_comment_owned_rows \
                   TO compat845_comment_reader",
                "SET ROLE compat845_comment_reader",
            ],
        )
        .await
        .expect("create an owned table and assume a reader role with SELECT privilege");

        // Act
        let reader = client
            .query_one(
                "SELECT current_user, \
                        has_table_privilege(current_user, \
                          'public.compat845_comment_owned_rows', 'SELECT'), \
                        pg_get_userbyid(relowner) \
                 FROM pg_catalog.pg_class \
                 WHERE oid = 'compat845_comment_owned_rows'::regclass",
                &[],
            )
            .await
            .expect("verify the active role can read the relation it does not own");
        let visible_id: i32 = client
            .query_one("SELECT id FROM compat845_comment_owned_rows", &[])
            .await
            .expect("read the row granted to the non-owner")
            .try_get(0)
            .expect("visible row id");
        let denied_comment = client
            .batch_execute(
                "COMMENT ON TABLE compat845_comment_owned_rows \
                 IS 'reader documentation'",
            )
            .await
            .expect_err("a reader with SELECT privilege must not add a table comment");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role after the denied comment");
        let unchanged_comment: Option<String> = client
            .query_one(
                "SELECT obj_description(\
                   'compat845_comment_owned_rows'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read comment metadata after the rejected write")
            .try_get(0)
            .expect("unchanged comment");

        // Assert
        assert_eq!(reader.get::<_, String>(0), "compat845_comment_reader");
        assert!(reader.get::<_, bool>(1));
        assert_eq!(reader.get::<_, String>(2), "compat845_comment_owner");
        assert_eq!(visible_id, 7);
        assert_eq!(db_error(&denied_comment).code().code(), "42501");
        assert_eq!(unchanged_comment, None);

        execute_statements(
            &client,
            &[
                "DROP TABLE compat845_comment_owned_rows",
                "DROP ROLE compat845_comment_reader",
                "DROP ROLE compat845_comment_owner",
            ],
        )
        .await
        .expect("remove the comment ownership fixture");
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_clear_relation_comment_when_the_object_is_recreated() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("comment_relation_cleanup").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat845_comment_cleanup_rows (id INT)",
                "COMMENT ON TABLE compat845_comment_cleanup_rows \
                   IS 'temporary documentation'",
            ],
        )
        .await
        .expect("create and annotate a relation before dependency cleanup");

        // Act
        execute_statements(
            &client,
            &[
                "DROP TABLE compat845_comment_cleanup_rows",
                "CREATE TABLE compat845_comment_cleanup_rows (id INT)",
            ],
        )
        .await
        .expect("drop and recreate the relation with the same name");
        let recreated_comment: Option<String> = client
            .query_one(
                "SELECT obj_description(\
                   'compat845_comment_cleanup_rows'::regclass, 'pg_class')",
                &[],
            )
            .await
            .expect("read metadata on the recreated relation")
            .try_get(0)
            .expect("recreated relation comment");

        // Assert
        assert_eq!(recreated_comment, None);

        drop(client);
        server.shutdown(connection).await;
    });
}
