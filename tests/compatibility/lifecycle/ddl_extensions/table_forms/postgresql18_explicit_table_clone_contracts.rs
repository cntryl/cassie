use super::*;

#[test]
fn should_copy_defaults_only_when_like_defaults_are_included() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("typed_table_defaults").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_typed_defaults_source (\
                   id INT DEFAULT 42, label TEXT DEFAULT 'fresh')",
                "CREATE TABLE compat_typed_defaults_included \
                   (LIKE compat_typed_defaults_source INCLUDING DEFAULTS)",
                "CREATE TABLE compat_typed_defaults_excluded \
                   (LIKE compat_typed_defaults_source EXCLUDING DEFAULTS)",
            ],
        )
        .await
        .expect("create source and LIKE tables using integer and text defaults");

        // Act
        let included = client
            .query_one(
                "INSERT INTO compat_typed_defaults_included DEFAULT VALUES \
                 RETURNING id, label",
                &[],
            )
            .await
            .expect("apply defaults copied by LIKE INCLUDING DEFAULTS");
        let excluded = client
            .query_one(
                "INSERT INTO compat_typed_defaults_excluded DEFAULT VALUES \
                 RETURNING id, label",
                &[],
            )
            .await
            .expect("insert into a LIKE copy with defaults explicitly excluded");

        // Assert
        assert_eq!(included.get::<_, i32>(0), 42);
        assert_eq!(included.get::<_, String>(1), "fresh");
        assert_eq!(excluded.get::<_, Option<i32>>(0), None);
        assert_eq!(excluded.get::<_, Option<String>>(1), None);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_create_table_like_with_including_constraints() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_named_check_constraint_clone").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_like_constraint_source (\
                   id INT, amount INT, \
                   CONSTRAINT compat_like_amount_positive CHECK (amount > 0), \
                   CONSTRAINT compat_like_id_unique UNIQUE (id)\
                 )",
            )
            .await
            .expect("create a source table with a CHECK and unique constraint");

        // Act
        let cloned = client
            .batch_execute(
                "CREATE TABLE compat_like_constraint_target \
                 (LIKE compat_like_constraint_source INCLUDING CONSTRAINTS)",
            )
            .await;
        if let Err(error) = &cloned {
            let database_error = db_error(error);
            panic!(
                "PostgreSQL 18.6 accepts CREATE TABLE LIKE ... INCLUDING CONSTRAINTS; \
                 Cassie returned DbError code={}, message={:?}, position={:?}",
                database_error.code().code(),
                database_error.message(),
                database_error.position()
            );
        }
        cloned.expect("create a LIKE copy that includes CHECK constraints but not indexes");
        let copied_check = client
            .query_one(
                "SELECT conname, contype::TEXT, convalidated \
                 FROM pg_catalog.pg_constraint \
                 WHERE conrelid = 'compat_like_constraint_target'::regclass \
                   AND conname = 'compat_like_amount_positive'",
                &[],
            )
            .await
            .expect("inspect the named CHECK copied to the target table");
        let target_indexes: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_index \
                 WHERE indrelid = 'compat_like_constraint_target'::regclass",
                &[],
            )
            .await
            .expect("count indexes copied to the target table")
            .try_get(0)
            .expect("target index count");
        let duplicate_ids = client
            .execute(
                "INSERT INTO compat_like_constraint_target VALUES (1, 4), (1, 5)",
                &[],
            )
            .await
            .expect("INCLUDING CONSTRAINTS must not copy the source unique index");
        let rejected_check = client
            .execute(
                "INSERT INTO compat_like_constraint_target VALUES (2, 0)",
                &[],
            )
            .await
            .expect_err("the copied named CHECK must reject nonpositive values");
        let rows = client
            .query(
                "SELECT id, amount FROM compat_like_constraint_target ORDER BY amount",
                &[],
            )
            .await
            .expect("read target rows after CHECK rejection");

        // Assert
        assert_eq!(
            copied_check.get::<_, String>(0),
            "compat_like_amount_positive"
        );
        assert_eq!(copied_check.get::<_, String>(1), "c");
        assert!(copied_check.get::<_, bool>(2));
        assert_eq!(target_indexes, 0);
        assert_eq!(duplicate_ids, 2);
        assert_eq!(db_error(&rejected_check).code().code(), "23514");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, i32>(1), 4);
        assert_eq!(rows[1].get::<_, i32>(0), 1);
        assert_eq!(rows[1].get::<_, i32>(1), 5);

        drop(client);
        server.shutdown(connection).await;
    });
}
