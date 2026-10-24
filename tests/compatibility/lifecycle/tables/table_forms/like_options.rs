use super::*;

struct LikeAllObservations {
    copied_default: tokio_postgres::Row,
    duplicate_all_label: tokio_postgres::Error,
    excluded_rows: u64,
    all_label_default: String,
    excluded_label_default: tokio_postgres::Row,
    all_identity: tokio_postgres::Row,
    all_checks: i64,
    excluded_checks: i64,
    all_indexes: i64,
    excluded_indexes: i64,
}

async fn read_like_all_catalog(
    client: &tokio_postgres::Client,
) -> (
    String,
    tokio_postgres::Row,
    tokio_postgres::Row,
    i64,
    i64,
    i64,
    i64,
) {
    let included_label = client
        .query_one(
            "SELECT column_default FROM information_schema.columns \
             WHERE table_name = 'compat_like_options_all' AND column_name = 'label'",
            &[],
        )
        .await
        .expect("inspect included column default")
        .get(0);
    let excluded_label_default = client
        .query_one(
            "SELECT column_default FROM information_schema.columns \
             WHERE table_name = 'compat_like_options_excluded' AND column_name = 'label'",
            &[],
        )
        .await
        .expect("inspect excluded default metadata");
    let all_identity = client
        .query_one(
            "SELECT is_identity FROM information_schema.columns \
             WHERE table_schema = 'public' AND table_name = 'compat_like_options_all' \
               AND column_name = 'id'",
            &[],
        )
        .await
        .expect("inspect included identity metadata");
    let all_checks = count_like_constraints(client, "compat_like_options_all").await;
    let excluded_checks = count_like_constraints(client, "compat_like_options_excluded").await;
    let all_indexes = count_like_indexes(client, "compat_like_options_all").await;
    let excluded_indexes = count_like_indexes(client, "compat_like_options_excluded").await;
    (
        included_label,
        excluded_label_default,
        all_identity,
        all_checks,
        excluded_checks,
        all_indexes,
        excluded_indexes,
    )
}

async fn count_like_constraints(client: &tokio_postgres::Client, table: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*) FROM information_schema.table_constraints \
             WHERE table_schema = 'public' AND table_name = $1::TEXT \
               AND constraint_type = 'CHECK'",
            &[&table],
        )
        .await
        .expect("count copied CHECK constraints")
        .get(0)
}

async fn count_like_indexes(client: &tokio_postgres::Client, table: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*) FROM pg_indexes \
             WHERE schemaname = 'public' AND tablename = $1::TEXT",
            &[&table],
        )
        .await
        .expect("count copied indexes")
        .get(0)
}

async fn observe_like_all_copy(client: &tokio_postgres::Client) -> LikeAllObservations {
    let copied_default = client
        .query_one(
            "INSERT INTO compat_like_options_all (id) VALUES (1) RETURNING label",
            &[],
        )
        .await
        .expect("INCLUDING ALL should copy the source default");
    let duplicate_all_label = client
        .execute(
            "INSERT INTO compat_like_options_all (id, label) VALUES (2, 'template')",
            &[],
        )
        .await
        .expect_err("INCLUDING ALL should copy the unique index");
    let excluded_rows = client
        .execute(
            "INSERT INTO compat_like_options_excluded (id, label) \
             VALUES (1, 'blocked'), (1, 'blocked')",
            &[],
        )
        .await
        .expect("EXCLUDING options should omit defaults, checks, and indexes");
    let (
        all_label_default,
        excluded_label_default,
        all_identity,
        all_checks,
        excluded_checks,
        all_indexes,
        excluded_indexes,
    ) = read_like_all_catalog(client).await;
    LikeAllObservations {
        copied_default,
        duplicate_all_label,
        excluded_rows,
        all_label_default,
        excluded_label_default,
        all_identity,
        all_checks,
        excluded_checks,
        all_indexes,
        excluded_indexes,
    }
}

#[test]
fn should_apply_like_all_with_explicit_exclusions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_all_explicit_exclusions").await;
        let (client, connection) = server.connect().await;
        for statement in [
            "CREATE TABLE compat_like_options_source (\
               id INT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,\
               label TEXT NOT NULL DEFAULT 'template',\
               CONSTRAINT compat_like_options_check CHECK (label <> 'blocked'),\
               UNIQUE (label)\
             )",
            "CREATE INDEX compat_like_options_source_label_idx \
             ON compat_like_options_source (label)",
            "CREATE TABLE compat_like_options_all (\
               LIKE compat_like_options_source INCLUDING ALL EXCLUDING IDENTITY\
             )",
            "CREATE TABLE compat_like_options_identity (\
               LIKE compat_like_options_source INCLUDING ALL\
             )",
            "CREATE TABLE compat_like_options_excluded (\
               LIKE compat_like_options_source INCLUDING ALL EXCLUDING DEFAULTS \
               EXCLUDING CONSTRAINTS EXCLUDING INDEXES EXCLUDING IDENTITY\
             )",
        ] {
            client
                .batch_execute(statement)
                .await
                .unwrap_or_else(|error| {
                    let error = db_error(&error);
                    panic!(
                        "statement {statement:?} failed with {}: {}",
                        error.code().code(),
                        error.message()
                    );
                });
        }

        // Act
        let observations = observe_like_all_copy(&client).await;
        let copied_identity: i32 = client
            .query_one(
                "INSERT INTO compat_like_options_identity (label) \
                 VALUES ('identity target') RETURNING id",
                &[],
            )
            .await
            .expect("INCLUDING ALL should create a target-owned identity sequence")
            .get(0);
        let source_identity: i32 = client
            .query_one(
                "INSERT INTO compat_like_options_source (label) \
                 VALUES ('identity source') RETURNING id",
                &[],
            )
            .await
            .expect("source identity sequence remains independent")
            .get(0);
        let copied_identity_generation: String = client
            .query_one(
                "SELECT identity_generation FROM information_schema.columns \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_like_options_identity' \
                   AND column_name = 'id'",
                &[],
            )
            .await
            .expect("inspect copied identity generation")
            .get(0);
        let copied_identity_indexes =
            count_like_indexes(&client, "compat_like_options_identity").await;

        // Assert
        assert_eq!(observations.copied_default.get::<_, String>(0), "template");
        assert_eq!(
            db_error(&observations.duplicate_all_label).code().code(),
            "23505"
        );
        assert_eq!(observations.excluded_rows, 2);
        assert_eq!(observations.all_label_default, "'template'");
        assert_eq!(
            observations
                .excluded_label_default
                .get::<_, Option<String>>(0),
            None
        );
        assert_eq!(observations.all_identity.get::<_, String>(0), "NO");
        assert_eq!(observations.all_checks, 1);
        assert_eq!(observations.excluded_checks, 0);
        assert_eq!(observations.all_indexes, 3);
        assert_eq!(observations.excluded_indexes, 0);
        assert_eq!(copied_identity, 1);
        assert_eq!(source_identity, 1);
        assert_eq!(copied_identity_generation, "ALWAYS");
        assert_eq!(copied_identity_indexes, 3);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_copy_scalar_indexes_when_like_includes_indexes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_including_indexes").await;
        let (client, connection) = server.connect().await;
        for statement in [
            "CREATE TABLE compat_like_indexes_source \
               (id INT UNIQUE NULLS NOT DISTINCT, label TEXT)",
            "CREATE INDEX compat_like_indexes_source_label_idx \
             ON compat_like_indexes_source (label)",
            "CREATE TABLE compat_like_indexes_target \
             (LIKE compat_like_indexes_source INCLUDING INDEXES)",
        ] {
            client
                .batch_execute(statement)
                .await
                .unwrap_or_else(|error| {
                    let error = db_error(&error);
                    panic!(
                        "statement {statement:?} failed with {}: {}",
                        error.code().code(),
                        error.message()
                    );
                });
        }

        // Act
        let copied_indexes = count_like_indexes(&client, "compat_like_indexes_target").await;
        client
            .batch_execute("INSERT INTO compat_like_indexes_target VALUES (1, 'first')")
            .await
            .expect("insert a row into the copied table");
        let duplicate_id = client
            .batch_execute("INSERT INTO compat_like_indexes_target VALUES (1, 'second')")
            .await
            .expect_err("INCLUDING INDEXES should preserve the source unique index");
        client
            .batch_execute("INSERT INTO compat_like_indexes_target VALUES (NULL, 'null key')")
            .await
            .expect("insert the first NULL into a NULLS NOT DISTINCT unique index");
        let duplicate_null = client
            .batch_execute("INSERT INTO compat_like_indexes_target VALUES (NULL, 'duplicate null')")
            .await
            .expect_err("copied NULLS NOT DISTINCT index must reject a duplicate NULL");
        client
            .batch_execute("INSERT INTO compat_like_indexes_source VALUES (NULL, 'source null')")
            .await
            .expect("target uniqueness reservation must not leak into the source");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(copied_indexes, 2);
        assert_eq!(db_error(&duplicate_id).code().code(), "23505");
        assert_eq!(db_error(&duplicate_null).code().code(), "23505");
    });
}

#[test]
fn should_preserve_composite_unique_key_order_when_like_includes_indexes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_composite_unique_indexes").await;
        let (client, connection) = server.connect().await;
        for statement in [
            "CREATE TABLE compat_like_composite_source \
               (a INT, b INT, CONSTRAINT compat_like_composite_pair UNIQUE (b, a))",
            "CREATE TABLE compat_like_composite_target \
               (LIKE compat_like_composite_source INCLUDING INDEXES)",
        ] {
            client
                .batch_execute(statement)
                .await
                .unwrap_or_else(|error| {
                    let error = db_error(&error);
                    panic!(
                        "statement {statement:?} failed with {}: {}",
                        error.code().code(),
                        error.message()
                    );
                });
        }

        // Act
        let unique_index_definition: String = client
            .query_one(
                "SELECT indexdef FROM pg_indexes \
                 WHERE schemaname = 'public' \
                   AND tablename = 'compat_like_composite_target' \
                   AND indexdef LIKE 'CREATE UNIQUE INDEX%'",
                &[],
            )
            .await
            .expect("inspect copied composite unique index")
            .get(0);
        client
            .batch_execute("INSERT INTO compat_like_composite_target VALUES (1, 2)")
            .await
            .expect("insert the first composite key");
        client
            .batch_execute("INSERT INTO compat_like_composite_target VALUES (2, 1)")
            .await
            .expect("different composite keys remain distinct");
        let duplicate = client
            .batch_execute("INSERT INTO compat_like_composite_target VALUES (1, 2)")
            .await
            .expect_err("copied composite unique constraint must reject a duplicate key");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            unique_index_definition.contains("(b, a)"),
            "copied key order was not preserved: {unique_index_definition}"
        );
        assert_eq!(db_error(&duplicate).code().code(), "23505");
    });
}

#[test]
fn should_copy_partial_unique_indexes_independently_when_like_includes_indexes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_partial_unique_indexes").await;
        let (client, connection) = server.connect().await;
        for statement in [
            "CREATE TABLE compat_like_partial_source (email TEXT, active BOOLEAN)",
            "CREATE UNIQUE INDEX compat_like_partial_source_active_email \
             ON compat_like_partial_source (email) WHERE active",
            "CREATE INDEX compat_like_partial_source_inactive_email \
             ON compat_like_partial_source (email) WHERE NOT active",
            "CREATE TABLE compat_like_partial_target \
             (LIKE compat_like_partial_source INCLUDING INDEXES)",
            "INSERT INTO compat_like_partial_target VALUES ('same@example.test', FALSE)",
            "INSERT INTO compat_like_partial_target VALUES ('same@example.test', FALSE)",
            "INSERT INTO compat_like_partial_target VALUES ('same@example.test', TRUE)",
        ] {
            client
                .batch_execute(statement)
                .await
                .unwrap_or_else(|error| {
                    let error = db_error(&error);
                    panic!(
                        "statement {statement:?} failed with {}: {}",
                        error.code().code(),
                        error.message()
                    );
                });
        }

        // Act
        let index_definition: String = client
            .query_one(
                "SELECT indexdef FROM pg_indexes \
                 WHERE schemaname = 'public' \
                   AND tablename = 'compat_like_partial_target' \
                   AND indexdef LIKE '%WHERE active'",
                &[],
            )
            .await
            .expect("inspect copied partial unique index")
            .get(0);
        let copied_index_names = client
            .query(
                "SELECT indexname FROM pg_indexes \
                 WHERE schemaname = 'public' \
                   AND tablename = 'compat_like_partial_target' \
                 ORDER BY indexname",
                &[],
            )
            .await
            .expect("list copied indexes")
            .into_iter()
            .map(|row| row.get::<_, String>(0))
            .collect::<Vec<_>>();
        let duplicate_target = client
            .batch_execute(
                "INSERT INTO compat_like_partial_target \
                 VALUES ('same@example.test', TRUE)",
            )
            .await
            .expect_err("copied partial unique index must reject a matching key");
        client
            .batch_execute(
                "INSERT INTO compat_like_partial_source \
                 VALUES ('same@example.test', TRUE)",
            )
            .await
            .expect("source uniqueness reservation must remain independent");
        let duplicate_source = client
            .batch_execute(
                "INSERT INTO compat_like_partial_source \
                 VALUES ('same@example.test', TRUE)",
            )
            .await
            .expect_err("the source index must continue enforcing its own uniqueness");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            index_definition
                .to_ascii_lowercase()
                .contains("where active"),
            "copied index predicate was not preserved: {index_definition}"
        );
        assert_eq!(copied_index_names.len(), 2);
        assert_ne!(copied_index_names[0], copied_index_names[1]);
        assert_eq!(db_error(&duplicate_target).code().code(), "23505");
        assert_eq!(db_error(&duplicate_source).code().code(), "23505");
    });
}

#[test]
fn should_exclude_only_defaults_when_like_includes_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("like_all_exclude_defaults_only").await;
        let (client, connection) = server.connect().await;
        for statement in [
            "CREATE TABLE compat_like_defaults_source (\
               label TEXT NOT NULL DEFAULT 'initial',\
               CONSTRAINT compat_like_defaults_check CHECK (label <> 'blocked')\
             )",
            "CREATE TABLE compat_like_defaults_target \
             (LIKE compat_like_defaults_source INCLUDING ALL EXCLUDING DEFAULTS)",
        ] {
            client
                .batch_execute(statement)
                .await
                .unwrap_or_else(|error| {
                    let error = db_error(&error);
                    panic!(
                        "statement {statement:?} failed with {}: {}",
                        error.code().code(),
                        error.message()
                    );
                });
        }

        // Act
        let column_default: Option<String> = client
            .query_one(
                "SELECT column_default FROM information_schema.columns \
                 WHERE table_schema = 'public' \
                   AND table_name = 'compat_like_defaults_target' \
                   AND column_name = 'label'",
                &[],
            )
            .await
            .expect("inspect excluded default")
            .get(0);
        let check_violation = client
            .batch_execute("INSERT INTO compat_like_defaults_target VALUES ('blocked')")
            .await
            .expect_err("EXCLUDING DEFAULTS must retain the copied CHECK constraint");
        let null_violation = client
            .batch_execute("INSERT INTO compat_like_defaults_target VALUES (NULL)")
            .await
            .expect_err("EXCLUDING DEFAULTS must retain NOT NULL");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(column_default, None);
        assert_eq!(db_error(&check_violation).code().code(), "23514");
        assert_eq!(db_error(&null_violation).code().code(), "23502");
    });
}
