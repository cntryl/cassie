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
                   id INT DEFAULT 42, label TEXT DEFAULT 'fresh', \
                   created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)",
                "CREATE TABLE compat_typed_defaults_included \
                   (LIKE compat_typed_defaults_source INCLUDING DEFAULTS)",
                "CREATE TABLE compat_typed_defaults_excluded \
                   (LIKE compat_typed_defaults_source INCLUDING DEFAULTS EXCLUDING DEFAULTS)",
                "CREATE TABLE compat_typed_defaults_reincluded \
                   (LIKE compat_typed_defaults_source EXCLUDING DEFAULTS INCLUDING DEFAULTS)",
                "CREATE TABLE compat_typed_defaults_implicit \
                   (LIKE compat_typed_defaults_source)",
            ],
        )
        .await
        .expect("create source and LIKE tables using integer and text defaults");

        // Act
        let included = client
            .query_one(
                "INSERT INTO compat_typed_defaults_included DEFAULT VALUES \
                 RETURNING id, label, created_at IS NOT NULL",
                &[],
            )
            .await
            .expect("apply defaults copied by LIKE INCLUDING DEFAULTS");
        let excluded = client
            .query_one(
                "INSERT INTO compat_typed_defaults_excluded DEFAULT VALUES \
                 RETURNING id, label, created_at IS NOT NULL",
                &[],
            )
            .await
            .expect("the final EXCLUDING DEFAULTS action omits copied defaults");
        let reincluded = client
            .query_one(
                "INSERT INTO compat_typed_defaults_reincluded DEFAULT VALUES \
                 RETURNING id, label, created_at IS NOT NULL",
                &[],
            )
            .await
            .expect("the final INCLUDING DEFAULTS action copies defaults");
        let implicit = client
            .query_one(
                "INSERT INTO compat_typed_defaults_implicit DEFAULT VALUES \
                 RETURNING id, label, created_at IS NOT NULL",
                &[],
            )
            .await
            .expect("defaults are excluded when no LIKE option is specified");

        // Assert
        assert_eq!(included.get::<_, i32>(0), 42);
        assert_eq!(included.get::<_, String>(1), "fresh");
        assert!(included.get::<_, bool>(2));
        assert_eq!(excluded.get::<_, Option<i32>>(0), None);
        assert_eq!(excluded.get::<_, Option<String>>(1), None);
        assert!(!excluded.get::<_, bool>(2));
        assert_eq!(reincluded.get::<_, i32>(0), 42);
        assert_eq!(reincluded.get::<_, String>(1), "fresh");
        assert!(reincluded.get::<_, bool>(2));
        assert_eq!(implicit.get::<_, Option<i32>>(0), None);
        assert_eq!(implicit.get::<_, Option<String>>(1), None);
        assert!(!implicit.get::<_, bool>(2));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_copy_not_null_when_like_is_used() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_columns_not_null").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_columns_source (\
                   id INT PRIMARY KEY, label TEXT UNIQUE, amount INT NOT NULL)",
                "CREATE TABLE compat_like_columns_target \
                   (LIKE compat_like_columns_source)",
            ],
        )
        .await
        .expect("create a column-only LIKE copy");

        // Act
        let null_id = client
            .execute(
                "INSERT INTO compat_like_columns_target VALUES (NULL, 'id', 4)",
                &[],
            )
            .await
            .expect_err("the source primary key's NOT NULL property is always copied");
        let null_amount = client
            .execute(
                "INSERT INTO compat_like_columns_target VALUES (2, 'amount', NULL)",
                &[],
            )
            .await
            .expect_err("the source NOT NULL property is always copied");

        // Assert
        assert_eq!(db_error(&null_id).code().code(), "23502");
        assert_eq!(db_error(&null_amount).code().code(), "23502");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_not_copy_unique_constraints_when_like_is_used() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_columns_unique").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_unique_source (\
                   id INT PRIMARY KEY, label TEXT UNIQUE, amount INT NOT NULL)",
                "CREATE TABLE compat_like_unique_target \
                   (LIKE compat_like_unique_source)",
            ],
        )
        .await
        .expect("create a column-only LIKE copy");

        // Act
        let inserted = client
            .execute(
                "INSERT INTO compat_like_unique_target VALUES \
                   (1, 'same', 2), (1, 'same', 3)",
                &[],
            )
            .await
            .expect("LIKE without INCLUDING INDEXES must not copy uniqueness");

        // Assert
        assert_eq!(inserted, 2);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_like_defaults_that_reference_source_owned_sequences() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_serial_default_dependency").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_like_serial_source (id SERIAL)")
            .await
            .expect("create a source table with an owned serial sequence");

        // Act
        let error = client
            .batch_execute(
                "CREATE TABLE compat_like_serial_target \
                   (LIKE compat_like_serial_source INCLUDING DEFAULTS)",
            )
            .await
            .expect_err("sequence defaults need source dependency tracking before copying");
        let sqlstate = db_error(&error).code().code().to_owned();
        let target_exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                   WHERE table_name = 'compat_like_serial_target')",
                &[],
            )
            .await
            .expect("check that rejection did not create the target")
            .get(0);
        let source_id: i32 = client
            .query_one(
                "INSERT INTO compat_like_serial_source DEFAULT VALUES RETURNING id",
                &[],
            )
            .await
            .expect("the rejected copy leaves the source sequence usable")
            .get(0);

        // Assert
        assert_eq!(sqlstate, "0A000");
        assert!(!target_exists);
        assert_eq!(source_id, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_not_copy_identity_when_like_includes_only_defaults() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_defaults_skip_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_like_identity_source (\
                   id INT GENERATED ALWAYS AS IDENTITY, label TEXT DEFAULT 'copied')",
                "CREATE TABLE compat_like_identity_target \
                   (LIKE compat_like_identity_source INCLUDING DEFAULTS)",
            ],
        )
        .await
        .expect("INCLUDING DEFAULTS is distinct from INCLUDING IDENTITY");

        // Act
        let inserted = client
            .query_one(
                "INSERT INTO compat_like_identity_target (id) \
                   VALUES (99) RETURNING id, label",
                &[],
            )
            .await
            .expect("the target has no copied identity generation but keeps ordinary defaults");

        // Assert
        assert_eq!(inserted.get::<_, i32>(0), 99);
        assert_eq!(inserted.get::<_, String>(1), "copied");

        drop(client);
        server.shutdown(connection).await;
    });
}

async fn create_like_constraint_tables(client: &tokio_postgres::Client) {
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

    let included = client
        .batch_execute(
            "CREATE TABLE compat_like_constraint_target \
             (LIKE compat_like_constraint_source INCLUDING CONSTRAINTS)",
        )
        .await;
    if let Err(error) = &included {
        let database_error = db_error(error);
        panic!(
            "PostgreSQL 18.6 accepts CREATE TABLE LIKE ... INCLUDING CONSTRAINTS; \
             Cassie returned DbError code={}, message={:?}, position={:?}",
            database_error.code().code(),
            database_error.message(),
            database_error.position()
        );
    }
    included.expect("create a LIKE copy that includes CHECK constraints but not indexes");
    client
        .batch_execute(
            "CREATE TABLE compat_like_constraint_excluded \
             (LIKE compat_like_constraint_source INCLUDING CONSTRAINTS \
                EXCLUDING CONSTRAINTS)",
        )
        .await
        .expect("the final EXCLUDING CONSTRAINTS action disables copied checks");
}

#[test]
fn should_create_table_like_with_including_constraints() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_named_check_constraint_clone").await;
        let (client, connection) = server.connect().await;
        create_like_constraint_tables(&client).await;

        // Act
        let copied_check = client
            .query_one(
                "SELECT conname, contype::TEXT, convalidated \
                 FROM pg_catalog.pg_constraint c \
                 JOIN pg_catalog.pg_class r ON r.relname = c.conrelid \
                 WHERE r.relname = 'compat_like_constraint_target' \
                   AND c.conname = 'compat_like_amount_positive'",
                &[],
            )
            .await
            .expect("inspect the named CHECK copied to the target table");
        let target_indexes: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_index i \
                 JOIN pg_catalog.pg_class r ON r.relname = i.indrelid \
                 WHERE r.relname = 'compat_like_constraint_target'",
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
        assert_eq!(
            db_error(&rejected_check).constraint(),
            Some("compat_like_amount_positive")
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, i32>(1), 4);
        assert_eq!(rows[1].get::<_, i32>(0), 1);
        assert_eq!(rows[1].get::<_, i32>(1), 5);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_multiple_named_checks_on_the_same_field_until_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_multiple_named_checks").await;
        let (client, connection) = server.connect().await;
        let create_error = client
            .batch_execute(
                "CREATE TABLE compat_like_multiple_check_source (\
                   amount INT, \
                   CONSTRAINT compat_like_amount_positive CHECK (amount > 0), \
                   CONSTRAINT compat_like_amount_bounded CHECK (amount < 10)\
                 )",
            )
            .await
            .expect_err("multiple CHECKs on one field remain unsupported");

        // Act
        let source_exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_name = 'compat_like_multiple_check_source')",
                &[],
            )
            .await
            .expect("check that rejected DDL did not publish the source relation")
            .get(0);

        // Assert
        assert_eq!(db_error(&create_error).code().code(), "0A000");
        assert!(!source_exists);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_multiple_inline_checks_on_the_same_field_until_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_multiple_inline_checks").await;
        let (client, connection) = server.connect().await;
        let create_error = client
            .batch_execute(
                "CREATE TABLE compat_like_multiple_inline_checks (\
                   amount INT CONSTRAINT compat_inline_amount_positive CHECK (amount > 0) \
                     CONSTRAINT compat_inline_amount_bounded CHECK (amount < 10)\
                 )",
            )
            .await
            .expect_err("multiple inline CHECKs on one field remain unsupported");

        // Act
        let relation_exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_name = 'compat_like_multiple_inline_checks')",
                &[],
            )
            .await
            .expect("check that rejected DDL did not publish the relation")
            .get(0);

        // Assert
        assert_eq!(db_error(&create_error).code().code(), "0A000");
        assert!(!relation_exists);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_not_publish_like_target_when_included_check_conflicts_with_declared_check() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_conflicting_copied_check").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_like_conflicting_check_source (\
                   amount INT, \
                   CONSTRAINT compat_source_positive CHECK (amount > 0)\
                 )",
            )
            .await
            .expect("create source table with a supported named CHECK");

        // Act
        let create_error = client
            .batch_execute(
                "CREATE TABLE compat_like_conflicting_check_target (\
                   LIKE compat_like_conflicting_check_source INCLUDING CONSTRAINTS, \
                   CONSTRAINT compat_target_bounded CHECK (amount < 10)\
                 )",
            )
            .await
            .expect_err("the copied and declared CHECKs conflict on the same field");
        let target_exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_name = 'compat_like_conflicting_check_target')",
                &[],
            )
            .await
            .expect("check that the rejected CREATE did not publish the target")
            .get(0);
        let source_checks: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_source_positive'",
                &[],
            )
            .await
            .expect("confirm source CHECK metadata survived target rejection")
            .get(0);

        // Assert
        assert_eq!(db_error(&create_error).code().code(), "0A000");
        assert!(!target_exists);
        assert_eq!(source_checks, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_exclude_checks_when_a_later_like_action_excludes_constraints() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_excluded_check_constraints").await;
        let (client, connection) = server.connect().await;
        create_like_constraint_tables(&client).await;

        // Act
        let excluded_checks: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint c \
                 JOIN pg_catalog.pg_class r ON r.relname = c.conrelid \
                 WHERE r.relname = 'compat_like_constraint_excluded' \
                   AND c.contype = 'c'",
                &[],
            )
            .await
            .expect("count checks excluded by the final LIKE option")
            .try_get(0)
            .expect("excluded check count");
        let invalid_check = client
            .execute(
                "INSERT INTO compat_like_constraint_excluded VALUES (3, 0)",
                &[],
            )
            .await
            .expect("the final EXCLUDING CONSTRAINTS action leaves the check uncopied");

        // Assert
        assert_eq!(excluded_checks, 0);
        assert_eq!(invalid_check, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_copy_checks_when_a_later_like_action_reincludes_constraints() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_reincluded_check_constraints").await;
        let (client, connection) = server.connect().await;
        create_like_constraint_tables(&client).await;
        client
            .batch_execute(
                "CREATE TABLE compat_like_constraint_reincluded \
                 (LIKE compat_like_constraint_source EXCLUDING CONSTRAINTS \
                    INCLUDING CONSTRAINTS)",
            )
            .await
            .expect("the final INCLUDING CONSTRAINTS action restores copied checks");

        // Act
        let copied_checks: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint c \
                 JOIN pg_catalog.pg_class r ON r.relname = c.conrelid \
                 WHERE r.relname = 'compat_like_constraint_reincluded' \
                   AND c.contype = 'c'",
                &[],
            )
            .await
            .expect("inspect the final included CHECK")
            .get(0);
        let rejected_check = client
            .execute(
                "INSERT INTO compat_like_constraint_reincluded VALUES (4, 0)",
                &[],
            )
            .await
            .expect_err("the final include action enforces the copied CHECK");

        // Assert
        assert_eq!(copied_checks, 1);
        assert_eq!(db_error(&rejected_check).code().code(), "23514");
        assert_eq!(
            db_error(&rejected_check).constraint(),
            Some("compat_like_amount_positive")
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_column_check_when_expression_references_another_field() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_column_check_references_other_field").await;
        let (client, connection) = server.connect().await;
        let create_error = client
            .batch_execute(
                "CREATE TABLE compat_like_check_owner_source (\
                   id INT, amount INT CONSTRAINT compat_like_positive_id \
                     CHECK (ID > 0)\
                 )",
            )
            .await
            .expect_err("column CHECK support is limited to its owning field");

        // Act
        let source_exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_name = 'compat_like_check_owner_source')",
                &[],
            )
            .await
            .expect("check that rejected CREATE did not publish the source relation")
            .get(0);

        // Assert
        assert_eq!(db_error(&create_error).code().code(), "0A000");
        assert!(!source_exists);

        drop(client);
        server.shutdown(connection).await;
    });
}
