use super::*;

#[test]
fn should_reject_partial_unique_index_inference_without_its_predicate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_index_without_inference_predicate").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_partial_target (\
                   id INT PRIMARY KEY, email TEXT, active BOOLEAN, version INT)",
                "CREATE UNIQUE INDEX compat_partial_target_email_active \
                   ON compat_partial_target(email) WHERE active",
                "INSERT INTO compat_partial_target VALUES \
                   (1, 'same@example.test', TRUE, 1)",
            ],
        )
        .await
        .expect("create a partial unique index and seed its predicate-matching row");

        // Act
        let rejected = client
            .execute(
                "INSERT INTO compat_partial_target (id, email, active, version) \
                 VALUES (2, 'same@example.test', TRUE, 2) \
                 ON CONFLICT (email) DO UPDATE SET version = EXCLUDED.version",
                &[],
            )
            .await;
        let sqlstate = rejected
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let stored = client
            .query_one(
                "SELECT id, version FROM compat_partial_target WHERE email = 'same@example.test'",
                &[],
            )
            .await
            .expect("read the row after the invalid conflict-target inference");

        // Assert
        assert_eq!(sqlstate.as_deref(), Some("42P10"));
        assert_eq!(stored.get::<_, i32>(0), 1);
        assert_eq!(stored.get::<_, i32>(1), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_missing_partial_predicates_before_examining_conflict_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_inference_before_rows").await;
        let (client, connection) = server.connect().await;
        execute_statements(&client, &[
            "CREATE TABLE partial_probe (id INT PRIMARY KEY, email TEXT, active BOOLEAN, version INT)",
            "CREATE UNIQUE INDEX partial_probe_key ON partial_probe(email) WHERE active",
        ]).await.expect("create partial unique arbiter");

        // Act
        let mut states = Vec::new();
        for action in ["DO NOTHING", "DO UPDATE SET version = EXCLUDED.version"] {
            for active in ["TRUE", "FALSE"] {
                let error = client.execute(&format!(
                    "INSERT INTO partial_probe VALUES (1, 'x', {active}, 2) ON CONFLICT (email) {action}"
                ), &[]).await.expect_err("missing inference predicate must reject an empty table");
                states.push(db_error(&error).code().code().to_owned());
            }
        }
        let count: i64 = client.query_one("SELECT count(*) FROM partial_probe", &[])
            .await.expect("read unchanged table").get(0);
        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert_eq!(states, vec!["42P10"; 4]);
        assert_eq!(count, 0);
    });
}

#[test]
fn should_preserve_valid_index_inference_given_partial_or_full_arbiters_when_filtering_candidates()
{
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("valid_partial_and_full_inference").await;
        let (client, connection) = server.connect().await;
        let mut observed = Vec::new();
        for full in [false, true] {
            let table = if full {
                "valid_partial_full_probe"
            } else {
                "valid_partial_only_probe"
            };
            let partial_index = format!("{table}_partial_key");
            for statement in [
                format!(
                    "CREATE TABLE {table} (id INT PRIMARY KEY, email TEXT, active BOOLEAN, version INT)"
                ),
                format!(
                    "CREATE UNIQUE INDEX {partial_index} ON {table}(email) WHERE active"
                ),
                format!("INSERT INTO {table} VALUES (1, 'x', TRUE, 1)"),
            ] {
                client
                    .batch_execute(&statement)
                    .await
                    .unwrap_or_else(|error| panic!("{statement}: {error:?}"));
            }
            if full {
                client.batch_execute(&format!(
                    "CREATE UNIQUE INDEX {table}_full_key ON {table}(email)"
                ))
                    .await.expect("create competing full unique index");
            }

            // Act
            for predicate in [" WHERE active", ""] {
                if !full && predicate.is_empty() { continue; }
                for action in ["DO NOTHING", "DO UPDATE SET version = EXCLUDED.version"] {
                    let changed = client.execute(&format!(
                        "INSERT INTO {table} VALUES (2, 'x', TRUE, 2) ON CONFLICT (email){predicate} {action}"
                    ), &[]).await.expect("valid index inference");
                    observed.push(changed);
                }
            }
        }
        let row = client.query_one("SELECT id, version FROM valid_partial_full_probe", &[])
            .await.expect("read final conflicting row");
        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert_eq!(observed, vec![0, 1, 0, 1, 0, 1]);
        assert_eq!(row.get::<_, i32>(0), 1);
        assert_eq!(row.get::<_, i32>(1), 2);
    });
}
