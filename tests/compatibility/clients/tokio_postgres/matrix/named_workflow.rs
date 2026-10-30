//! Named tokio-postgres read-model workflow for the v0.3 client contract.

use super::{db_error, CompatibilityServer};

const VIRTUAL_PARAMETER_CASES: &[(&str, &str, u32)] = &[
    (
        "bare",
        "SELECT table_name FROM information_schema.tables WHERE table_name = $1",
        25,
    ),
    (
        "alias",
        "SELECT t.table_name FROM information_schema.tables AS t WHERE t.table_name = $1",
        25,
    ),
    (
        "qualified",
        "SELECT table_name FROM information_schema.tables WHERE tables.table_name = $1",
        25,
    ),
    (
        "ordinary",
        "SELECT title FROM virtual_inference_control WHERE id = $1",
        23,
    ),
    (
        "name_collision",
        "SELECT table_name FROM tables WHERE table_name = $1",
        23,
    ),
];

async fn commit_named_row(client: &tokio_postgres::Client) {
    client.batch_execute("BEGIN").await.expect("begin write");
    client
        .execute(
            "INSERT INTO named_tokio_probe (id, title) VALUES ($1, $2)",
            &[&1_i32, &"alpha"],
        )
        .await
        .expect("write bound values");
    client.batch_execute("COMMIT").await.expect("commit write");
}

async fn roll_back_named_row(client: &tokio_postgres::Client) {
    client.batch_execute("BEGIN").await.expect("begin rollback");
    client
        .execute(
            "INSERT INTO named_tokio_probe (id, title) VALUES ($1, $2)",
            &[&2_i32, &"beta"],
        )
        .await
        .expect("stage a row to discard");
    client.batch_execute("ROLLBACK").await.expect("roll back");
}

fn report_named_tokio_results() {
    println!("tokio_postgres_catalog=named_tokio_probe");
    println!("tokio_postgres_joined=alpha");
    println!("tokio_postgres_transaction_row_count=1");
    println!("tokio_postgres_duplicate_sqlstate=23505");
    println!("tokio_postgres_missing_sqlstate=42P01");
    println!("tokio_postgres_after_duplicate=alpha");
    println!("tokio_postgres_after_missing=alpha");
    println!("tokio_postgres_same_session=true");
}

#[test]
fn should_infer_virtual_catalog_parameter_types_without_inventing_names() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build current-thread runtime");
    runtime.block_on(async {
        let server = CompatibilityServer::start("virtual_catalog_parameter_inference").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE virtual_inference_control (id INT, title TEXT)")
            .await
            .expect("create ordinary-table control");
        client
            .batch_execute("CREATE TABLE tables (table_name INT)")
            .await
            .expect("create a user table sharing the virtual relation's leaf name");

        // Act
        let mut results = Vec::new();
        for &(label, sql, expected_oid) in VIRTUAL_PARAMETER_CASES {
            let observed = match client.prepare(sql).await {
                Ok(statement) => statement.params().first().map_or_else(
                    || "no parameter descriptor".to_owned(),
                    |parameter| parameter.oid().to_string(),
                ),
                Err(error) => db_error(&error).code().code().to_owned(),
            };
            println!("virtual_inference_{label}={observed}");
            results.push((label, observed, expected_oid));
        }
        let unknown_column = client
            .prepare(
                "SELECT no_such_column FROM information_schema.tables \
                 WHERE table_name = $1::TEXT",
            )
            .await;
        let unknown_relation = client
            .prepare("SELECT $1::TEXT FROM information_schema.no_such_view")
            .await;
        let unknown_qualifier = client
            .prepare(
                "SELECT table_name FROM information_schema.tables \
                 WHERE bogus.table_name = $1::TEXT",
            )
            .await;
        println!(
            "virtual_inference_unknown_column={}",
            unknown_column
                .as_ref()
                .err()
                .map_or("accepted", |error| db_error(error).code().code())
        );
        println!(
            "virtual_inference_unknown_relation={}",
            unknown_relation
                .as_ref()
                .err()
                .map_or("accepted", |error| db_error(error).code().code())
        );
        println!(
            "virtual_inference_unknown_qualifier={}",
            unknown_qualifier
                .as_ref()
                .err()
                .map_or("accepted", |error| db_error(error).code().code())
        );

        // Assert
        assert_eq!(
            db_error(&unknown_column.expect_err("reject an unknown virtual column"))
                .code()
                .code(),
            "42703"
        );
        assert_eq!(
            db_error(&unknown_relation.expect_err("reject an unknown virtual relation"))
                .code()
                .code(),
            "42P01"
        );
        assert_eq!(
            db_error(&unknown_qualifier.expect_err("reject an unrelated qualifier"))
                .code()
                .code(),
            "42703"
        );
        for (label, observed, expected_oid) in results {
            assert_eq!(observed, expected_oid.to_string(), "{label}");
        }

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_named_tokio_postgres_session_through_error_recovery() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build current-thread runtime");
    runtime.block_on(async {
        let server = CompatibilityServer::start("named_tokio_postgres_workflow").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE named_tokio_probe (id INT PRIMARY KEY, title TEXT NOT NULL UNIQUE)",
            )
            .await
            .expect("create read-model table");
        let initial_pid: i64 = client
            .query_one("SELECT pg_backend_pid()::BIGINT", &[])
            .await
            .expect("read backend identity")
            .get(0);

        // Act
        commit_named_row(&client).await;
        let catalog: String = client
            .query_one(
                "SELECT table_name FROM information_schema.tables WHERE table_name = $1",
                &[&"named_tokio_probe"],
            )
            .await
            .expect("discover the read-model table")
            .get(0);
        let joined: String = client
            .query_one(
                "SELECT l.title FROM named_tokio_probe AS l \
                 JOIN named_tokio_probe AS r ON l.id = r.id WHERE r.id = $1",
                &[&1_i32],
            )
            .await
            .expect("execute a parameterized relational read")
            .get(0);
        roll_back_named_row(&client).await;
        let count: i64 = client
            .query_one("SELECT COUNT(*) FROM named_tokio_probe", &[])
            .await
            .expect("read the committed row count")
            .get(0);
        let duplicate = client
            .execute(
                "INSERT INTO named_tokio_probe (id, title) VALUES ($1, $2)",
                &[&3_i32, &"alpha"],
            )
            .await
            .expect_err("reject the duplicate title");
        let after_duplicate: String = client
            .query_one(
                "SELECT title FROM named_tokio_probe WHERE id = $1",
                &[&1_i32],
            )
            .await
            .expect("query successfully after unique error")
            .get(0);
        let pid_after_duplicate: i64 = client
            .query_one("SELECT pg_backend_pid()::BIGINT", &[])
            .await
            .expect("read backend identity after unique error")
            .get(0);
        let missing = client
            .query("SELECT title FROM named_tokio_missing", &[])
            .await
            .expect_err("reject the missing relation");
        let after_missing: String = client
            .query_one(
                "SELECT title FROM named_tokio_probe WHERE id = $1",
                &[&1_i32],
            )
            .await
            .expect("query successfully after missing relation")
            .get(0);
        let pid_after_missing: i64 = client
            .query_one("SELECT pg_backend_pid()::BIGINT", &[])
            .await
            .expect("read backend identity after missing relation")
            .get(0);

        // Assert
        assert_eq!(catalog, "named_tokio_probe");
        assert_eq!(joined, "alpha");
        assert_eq!(count, 1);
        assert_eq!(db_error(&duplicate).code().code(), "23505");
        assert_eq!(db_error(&missing).code().code(), "42P01");
        assert_eq!(after_duplicate, "alpha");
        assert_eq!(after_missing, "alpha");
        assert!(initial_pid > 0);
        assert_eq!(initial_pid, pid_after_duplicate);
        assert_eq!(initial_pid, pid_after_missing);
        report_named_tokio_results();
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_resolve_named_schema_before_public_on_one_backend() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build current-thread runtime");
    runtime.block_on(async {
        let server = CompatibilityServer::start("named_schema_workflow").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SCHEMA named_tokio_scope")
            .await
            .expect("create the named schema");
        for sql in [
            "CREATE TABLE public.named_tokio_shadow (label TEXT NOT NULL)",
            "CREATE TABLE named_tokio_scope.named_tokio_shadow (label TEXT NOT NULL)",
            "INSERT INTO public.named_tokio_shadow VALUES ('public-value')",
            "INSERT INTO named_tokio_scope.named_tokio_shadow VALUES ('named-value')",
        ] {
            client
                .batch_execute(sql)
                .await
                .expect("create shadow fixture");
        }
        let initial_pid: i64 = client
            .query_one("SELECT pg_backend_pid()::BIGINT", &[])
            .await
            .expect("read initial backend identity")
            .get(0);

        // Act
        client
            .batch_execute("SET search_path TO named_tokio_scope, public")
            .await
            .expect("select the named schema");
        let named_schema: String = client
            .query_one("SELECT current_schema()::TEXT", &[])
            .await
            .expect("read named current schema")
            .get(0);
        let named_value: String = client
            .query_one("SELECT label FROM named_tokio_shadow", &[])
            .await
            .expect("read the shadow from the named schema")
            .get(0);
        let discovered_schema: String = client
            .query_one(
                "SELECT table_schema FROM information_schema.tables \
                 WHERE table_name = $1 AND table_schema = $2",
                &[&"named_tokio_shadow", &"named_tokio_scope"],
            )
            .await
            .expect("discover the named schema table")
            .get(0);
        client
            .batch_execute("SET search_path TO public")
            .await
            .expect("restore public search path");
        let public_schema: String = client
            .query_one("SELECT current_schema()::TEXT", &[])
            .await
            .expect("read public current schema")
            .get(0);
        let public_value: String = client
            .query_one("SELECT label FROM named_tokio_shadow", &[])
            .await
            .expect("read the public shadow")
            .get(0);
        let final_pid: i64 = client
            .query_one("SELECT pg_backend_pid()::BIGINT", &[])
            .await
            .expect("read final backend identity")
            .get(0);

        // Assert
        assert_eq!(named_schema, "named_tokio_scope");
        assert_eq!(named_value, "named-value");
        assert_eq!(discovered_schema, "named_tokio_scope");
        assert_eq!(public_schema, "public");
        assert_eq!(public_value, "public-value");
        assert!(initial_pid > 0);
        assert_eq!(initial_pid, final_pid);
        println!("tokio_postgres_named_schema=named_tokio_scope");
        println!("tokio_postgres_named_shadow=named-value");
        println!("tokio_postgres_named_discovery=named_tokio_scope");
        println!("tokio_postgres_public_schema=public");
        println!("tokio_postgres_public_shadow=public-value");
        println!("tokio_postgres_schema_same_session=true");
        drop(client);
        server.shutdown(connection).await;
    });
}
