use super::*;

#[test]
fn should_update_rows_from_a_joined_source() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("update_from_join").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mutation_target (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_mutation_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mutation_target VALUES (1, 'old-a'), (2, 'old-b'), (3, 'keep')",
                "INSERT INTO compat_mutation_source VALUES (1, 'new-a'), (2, 'new-b')",
            ],
        )
        .await
        .expect("create joined-mutation fixtures");

        // Act
        let updated = client
            .query(
                "UPDATE compat_mutation_target AS target \
                 SET label = source.label \
                 FROM compat_mutation_source AS source \
                 WHERE target.id = source.id \
                 RETURNING target.id, target.label",
                &[],
            )
            .await
            .expect("update from a joined source");
        // Assert
        let mut updated_rows = updated
            .iter()
            .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
            .collect::<Vec<_>>();
        updated_rows.sort();
        assert_eq!(
            updated_rows,
            vec![(1, "new-a".to_string()), (2, "new-b".to_string())]
        );
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_delete_rows_using_a_joined_source() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("delete_using_join").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_delete_target (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_delete_source (id INT PRIMARY KEY)",
                "INSERT INTO compat_delete_target VALUES (1, 'keep'), (2, 'delete')",
                "INSERT INTO compat_delete_source VALUES (2)",
            ],
        )
        .await
        .expect("create DELETE USING fixture rows");

        // Act
        let deleted = client
            .query(
                "DELETE FROM compat_delete_target AS target \
                 USING compat_delete_source AS source \
                 WHERE target.id = source.id RETURNING target.id",
                &[],
            )
            .await
            .expect("delete target rows matched by source");
        let remaining = client
            .query("SELECT id FROM compat_delete_target ORDER BY id", &[])
            .await
            .expect("read rows after DELETE USING");

        // Assert
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].get::<_, i32>(0), 2);
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_truncate_a_relation_to_an_empty_result() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("truncate_relation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_truncate_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_truncate_rows VALUES (1), (2)",
            ],
        )
        .await
        .expect("create rows to truncate");

        // Act
        client
            .batch_execute("TRUNCATE compat_truncate_rows")
            .await
            .expect("truncate relation");
        let count: i64 = client
            .query_one("SELECT count(*) FROM compat_truncate_rows", &[])
            .await
            .expect("count rows after truncation")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_merge_action_values_through_tokio_postgres() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("merge_action_returning").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_merge_target (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_merge_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_merge_target VALUES (1, 'old')",
                "INSERT INTO compat_merge_source VALUES (1, 'updated'), (2, 'inserted')",
            ],
        )
        .await
        .expect("create merge fixtures");

        // Act
        let actions = client
            .query(
                "MERGE INTO compat_merge_target AS target \
                 USING compat_merge_source AS source ON target.id = source.id \
                 WHEN MATCHED THEN UPDATE SET label = source.label \
                 WHEN NOT MATCHED THEN INSERT (id, label) VALUES (source.id, source.label) \
                 RETURNING merge_action(), target.id, target.label",
                &[],
            )
            .await
            .expect("execute MERGE and return action values");
        let mut observed = actions
            .iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    row.get::<_, i32>(1),
                    row.get::<_, String>(2),
                )
            })
            .collect::<Vec<_>>();
        observed.sort_by_key(|row| row.1);

        // Assert
        assert_eq!(
            observed,
            vec![
                ("UPDATE".to_string(), 1, "updated".to_string()),
                ("INSERT".to_string(), 2, "inserted".to_string())
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_return_prior_row_for_conflict_update() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conflict_returning_old_new").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_conflict_old_new (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_conflict_old_new VALUES (1, 'first')",
            ],
        )
        .await
        .expect("create OLD/NEW conflict RETURNING fixture");

        // Act
        let updated = client
            .query(
                "INSERT INTO compat_conflict_old_new AS target (id, label) VALUES (1, 'second') \
                 ON CONFLICT (id) DO UPDATE SET label = EXCLUDED.label \
                 RETURNING WITH (OLD AS old_row, NEW AS new_row) \
                   old_row.id, old_row.label, new_row.id, new_row.label",
                &[],
            )
            .await
            .expect("return OLD and NEW on a conflict update");

        // Assert
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].get::<_, i32>(0), 1);
        assert_eq!(updated[0].get::<_, String>(1), "first");
        assert_eq!(updated[0].get::<_, i32>(2), 1);
        assert_eq!(updated[0].get::<_, String>(3), "second");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_delete_merge_targets_missing_from_the_source() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("merge_not_matched_by_source").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_merge_by_source_target (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_merge_by_source_source (id INT, label TEXT)",
                "INSERT INTO compat_merge_by_source_target VALUES (1, 'old'), (2, 'remove')",
                "INSERT INTO compat_merge_by_source_source VALUES (1, 'updated')",
            ],
        )
        .await
        .expect("create MERGE BY SOURCE fixtures");

        // Act
        let actions = client
            .query(
                "MERGE INTO compat_merge_by_source_target AS target \
                 USING compat_merge_by_source_source AS source ON target.id = source.id \
                 WHEN MATCHED THEN UPDATE SET label = source.label \
                 WHEN NOT MATCHED BY SOURCE THEN DELETE \
                 RETURNING merge_action(), target.id",
                &[],
            )
            .await
            .expect("update matched rows and delete target-only rows");
        let mut observed = actions
            .iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, i32>(1)))
            .collect::<Vec<_>>();
        observed.sort_by_key(|row| row.1);

        // Assert
        assert_eq!(
            observed,
            vec![("UPDATE".to_owned(), 1), ("DELETE".to_owned(), 2)]
        );
        let remaining = client
            .query(
                "SELECT id, label FROM compat_merge_by_source_target ORDER BY id",
                &[],
            )
            .await
            .expect("read the target after MERGE BY SOURCE");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].get::<_, i32>(0), 1);
        assert_eq!(remaining[0].get::<_, String>(1), "updated");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_merge_that_would_modify_a_target_row_twice() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("merge_cardinality_violation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_merge_cardinality_target (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_merge_cardinality_target VALUES (1, 'old')",
            ],
        )
        .await
        .expect("create MERGE cardinality fixture");

        // Act
        let conflict = client
            .execute(
                "MERGE INTO compat_merge_cardinality_target AS target \
                 USING (VALUES (1, 'first'), (1, 'second')) AS source(id, label) \
                 ON target.id = source.id \
                 WHEN MATCHED THEN UPDATE SET label = source.label",
                &[],
            )
            .await
            .expect_err("a second source match must not modify the same target again");

        // Assert
        assert_eq!(db_error(&conflict).code().code(), "21000");
        let stored_label = client
            .query_one(
                "SELECT label FROM compat_merge_cardinality_target WHERE id = 1",
                &[],
            )
            .await
            .expect("read the target after the rejected MERGE")
            .get::<_, String>(0);
        assert_eq!(stored_label, "old", "the failed MERGE is atomic");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_return_only_rows_changed_by_a_named_conditional_conflict_target() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conditional_conflict_target").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_conflict_targets (id INT PRIMARY KEY, email TEXT, version INT, \
                   CONSTRAINT compat_conflict_email_key UNIQUE (email))",
                "INSERT INTO compat_conflict_targets VALUES (1, 'a@example.com', 1)",
            ],
        )
            .await
            .expect("create named conflict constraint and seed row");

        // Act
        let changed = client
            .query(
                "INSERT INTO compat_conflict_targets (id, email, version) \
                 VALUES (2, 'a@example.com', 2) \
                 ON CONFLICT ON CONSTRAINT compat_conflict_email_key \
                 DO UPDATE SET version = EXCLUDED.version \
                 WHERE compat_conflict_targets.version < EXCLUDED.version \
                 RETURNING id, version",
                &[],
            )
            .await
            .expect("update the row selected by named constraint");
        let unchanged = client
            .query(
                "INSERT INTO compat_conflict_targets (id, email, version) \
                 VALUES (3, 'a@example.com', 1) \
                 ON CONFLICT ON CONSTRAINT compat_conflict_email_key \
                 DO UPDATE SET version = EXCLUDED.version \
                 WHERE compat_conflict_targets.version < EXCLUDED.version \
                 RETURNING id, version",
                &[],
            )
            .await
            .expect("skip stale conflict update");
        let stored_row = client
            .query_one(
                "SELECT id, version FROM compat_conflict_targets WHERE email = 'a@example.com'",
                &[],
            )
            .await
            .expect("read target after conflict statements");
        let stored = (stored_row.get::<_, i32>(0), stored_row.get::<_, i32>(1));

        // Assert
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].get::<_, i32>(0), 1);
        assert_eq!(changed[0].get::<_, i32>(1), 2);
        assert!(unchanged.is_empty(), "a skipped UPDATE has no RETURNING row");
        assert_eq!(stored, (1, 2));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_rollback_data_modifying_cte_on_later_error() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("modifying_cte_atomicity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_cte_mutations (id INT PRIMARY KEY, label TEXT)")
            .await
            .expect("create CTE target");

        // Act
        let returned = client
            .query(
                "WITH inserted AS ( \
                   INSERT INTO compat_cte_mutations VALUES (1, 'created') RETURNING id, label \
                 ) SELECT id, label FROM inserted",
                &[],
            )
            .await
            .expect("read the data-modifying CTE RETURNING relation");
        let failed = client
            .query(
                "WITH inserted AS ( \
                   INSERT INTO compat_cte_mutations VALUES (2, 'rolled-back') RETURNING id \
                 ) SELECT id / 0 FROM inserted",
                &[],
            )
            .await
            .expect_err("later error should fail the complete CTE statement");
        let rows = client
            .query(
                "SELECT id, label FROM compat_cte_mutations ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after failed CTE");

        // Assert
        assert_eq!(returned.len(), 1);
        assert_eq!(returned[0].get::<_, i32>(0), 1);
        assert_eq!(returned[0].get::<_, String>(1), "created");
        assert_eq!(db_error(&failed).code().code(), "22012");
        assert_eq!(rows.len(), 1, "failed CTE must not publish its insert");
        assert_eq!(rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_honor_session_local_temporary_table_drop() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("temporary_table_lifecycle").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TEMP TABLE compat_temp_rows (id INT) ON COMMIT PRESERVE ROWS",
                "INSERT INTO compat_temp_rows VALUES (7)",
            ],
        )
        .await
        .expect("create session-local temp table");

        // Act
        let private_rows = owner
            .query("SELECT id FROM compat_temp_rows", &[])
            .await
            .expect("read temp table in owner session");
        let invisible = observer
            .query("SELECT id FROM compat_temp_rows", &[])
            .await
            .expect_err("another session cannot see a temp table");
        execute_statements(
            &owner,
            &[
                "BEGIN",
                "CREATE TEMP TABLE compat_temp_drop (id INT) ON COMMIT DROP",
                "INSERT INTO compat_temp_drop VALUES (8)",
                "COMMIT",
            ],
        )
        .await
        .expect("drop ON COMMIT relation at transaction completion");
        let dropped = owner
            .query("SELECT id FROM compat_temp_drop", &[])
            .await
            .expect_err("ON COMMIT DROP table must be absent");

        // Assert
        assert_eq!(private_rows.len(), 1);
        assert_eq!(private_rows[0].get::<_, i32>(0), 7);
        assert_eq!(db_error(&invisible).code().code(), "42P01");
        assert_eq!(db_error(&dropped).code().code(), "42P01");

        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}

#[test]
fn should_refresh_a_materialized_view_with_no_data() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("materialized_view_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mv_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mv_source VALUES (1, 'one'), (2, 'two')",
                "CREATE MATERIALIZED VIEW compat_mv_rows (item_id, item_label) AS \
                   SELECT id, label FROM compat_mv_source WITH NO DATA",
            ],
        )
        .await
        .expect("create materialized view without initial data");

        // Act
        let unpopulated = client
            .query_one("SELECT count(*) FROM compat_mv_rows", &[])
            .await
            .expect_err("a materialized view created WITH NO DATA is not scannable");
        client
            .batch_execute("REFRESH MATERIALIZED VIEW compat_mv_rows")
            .await
            .expect("refresh materialized view");
        let populated = client
            .query(
                "SELECT item_id, item_label FROM compat_mv_rows ORDER BY item_id",
                &[],
            )
            .await
            .expect("read refreshed materialized view");

        // Assert
        assert_eq!(db_error(&unpopulated).code().code(), "55000");
        assert_eq!(populated.len(), 2);
        assert_eq!(populated[0].get::<_, i32>(0), 1);
        assert_eq!(populated[0].get::<_, String>(1), "one");
        assert_eq!(populated[1].get::<_, i32>(0), 2);
        assert_eq!(populated[1].get::<_, String>(1), "two");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_truncate_referencing_rows_with_cascade() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("truncate_cascade_foreign_key").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_truncate_cascade_parent (id INT PRIMARY KEY)",
                "CREATE TABLE compat_truncate_cascade_child (\
                   id INT PRIMARY KEY, parent_id INT REFERENCES compat_truncate_cascade_parent(id))",
                "INSERT INTO compat_truncate_cascade_parent VALUES (1), (2)",
                "INSERT INTO compat_truncate_cascade_child VALUES (10, 1), (20, 2)",
            ],
        )
        .await
        .expect("create parent and referencing rows");

        // Act
        let truncated = client
            .batch_execute("TRUNCATE TABLE compat_truncate_cascade_parent CASCADE")
            .await;
        let remaining = if truncated.is_ok() {
            Some(
                client
                .query_one(
                    "SELECT (SELECT count(*) FROM compat_truncate_cascade_parent), \
                     (SELECT count(*) FROM compat_truncate_cascade_child)",
                    &[],
                )
                .await
                .expect("count rows after cascading truncate"),
            )
        } else {
            None
        };

        // Assert
        let truncate_sqlstate = truncated.as_ref().err().map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;

        assert!(
            truncated.is_ok(),
            "TRUNCATE CASCADE should remove dependent rows; SQLSTATE: {truncate_sqlstate:?}"
        );
        let remaining = remaining.expect("successful TRUNCATE CASCADE should produce row counts");
        assert_eq!(remaining.get::<_, i64>(0), 0);
        assert_eq!(remaining.get::<_, i64>(1), 0);
    });
}

#[test]
fn should_preserve_enum_order_through_alterations() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_value_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_mood AS ENUM ('sad', 'happy')",
                "CREATE TABLE compat_enum_rows (mood compat_mood)",
                "INSERT INTO compat_enum_rows VALUES ('sad'), ('happy')",
            ],
        )
        .await
        .expect("create enum and typed rows");

        // Act
        client
            .batch_execute("ALTER TYPE compat_mood ADD VALUE 'neutral' BEFORE 'happy'")
            .await
            .expect("add enum value before an existing value");
        client
            .batch_execute("ALTER TYPE compat_mood RENAME VALUE 'sad' TO 'down'")
            .await
            .expect("rename an enum label without changing its ordering");
        client
            .batch_execute("INSERT INTO compat_enum_rows VALUES ('neutral')")
            .await
            .expect("store the new enum label after commit");
        let values = client
            .query("SELECT mood::text FROM compat_enum_rows ORDER BY mood", &[])
            .await
            .expect("read enum values in declaration order");
        let dependent_drop = client
            .batch_execute("DROP TYPE compat_mood")
            .await
            .expect_err("a table column depends on the enum type");
        execute_statements(
            &client,
            &["DROP TABLE compat_enum_rows", "DROP TYPE compat_mood"],
        )
        .await
        .expect("remove the dependent table and enum type");
        let remaining_types: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_type WHERE typname = 'compat_mood'",
                &[],
            )
            .await
            .expect("check enum type cleanup")
            .try_get(0)
            .expect("type count");

        // Assert
        assert_eq!(
            values
                .iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>(),
            vec!["down", "neutral", "happy"]
        );
        assert_eq!(db_error(&dependent_drop).code().code(), "2BP01");
        assert_eq!(remaining_types, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_domain_constraint_lifecycle() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("domain_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE DOMAIN compat_positive AS INT \
                   CONSTRAINT compat_positive_check CHECK (VALUE > 0)",
                "CREATE TABLE compat_domain_rows (id INT PRIMARY KEY, amount compat_positive)",
                "INSERT INTO compat_domain_rows VALUES (1, 12)",
            ],
        )
        .await
        .expect("create constrained domain and typed row");

        // Act
        let invalid = client
            .execute("INSERT INTO compat_domain_rows VALUES (2, -1)", &[])
            .await
            .expect_err("domain check rejects a non-positive value");
        client
            .batch_execute("ALTER DOMAIN compat_positive RENAME TO compat_positive_amount")
            .await
            .expect("rename a domain used by a table column");
        let value: i32 = client
            .query_one(
                "SELECT amount::INT FROM compat_domain_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read value through renamed domain")
            .try_get(0)
            .expect("base integer value");
        let dependent_drop = client
            .batch_execute("DROP DOMAIN compat_positive_amount")
            .await
            .expect_err("RESTRICT prevents dropping a referenced domain");
        execute_statements(
            &client,
            &[
                "DROP TABLE compat_domain_rows",
                "DROP DOMAIN compat_positive_amount",
            ],
        )
        .await
        .expect("remove domain dependency and domain");

        // Assert
        assert_eq!(db_error(&invalid).code().code(), "23514");
        assert_eq!(value, 12);
        assert_eq!(db_error(&dependent_drop).code().code(), "2BP01");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_publish_sequence_restart_options_through_sql() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_options").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE SEQUENCE compat_order_sequence START WITH 7 INCREMENT BY 3 \
                   MINVALUE 1 MAXVALUE 10 CYCLE",
            )
            .await
            .expect("create configured sequence");

        // Act
        let values = client
            .query(
                "SELECT nextval('compat_order_sequence'), currval('compat_order_sequence')",
                &[],
            )
            .await
            .expect("read next and current sequence values");
        client
            .batch_execute("ALTER SEQUENCE compat_order_sequence RESTART WITH 4")
            .await
            .expect("restart sequence at selected value");
        let restarted: i64 = client
            .query_one("SELECT nextval('compat_order_sequence')", &[])
            .await
            .expect("read restarted sequence value")
            .try_get(0)
            .expect("sequence value");

        // Assert
        assert_eq!(values[0].get::<_, i64>(0), 7);
        assert_eq!(values[0].get::<_, i64>(1), 7);
        assert_eq!(restarted, 4);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_identity_generation_sequence_options() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("identity_generation").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_identity_rows (\
                   id BIGINT GENERATED BY DEFAULT AS IDENTITY (START WITH 10 INCREMENT BY 2), \
                   label TEXT)",
            )
            .await
            .expect("create identity column with sequence options");

        // Act
        let first_generated: i64 = client
            .query_one(
                "INSERT INTO compat_identity_rows (label) VALUES ('generated-first') RETURNING id",
                &[],
            )
            .await
            .expect("insert the first generated identity")
            .try_get(0)
            .expect("identity value");
        let explicit: i64 = client
            .query_one(
                "INSERT INTO compat_identity_rows (id, label) VALUES (40, 'explicit') RETURNING id",
                &[],
            )
            .await
            .expect("accept an explicit value for BY DEFAULT identity")
            .try_get(0)
            .expect("explicit identity value");
        let second_generated: i64 = client
            .query_one(
                "INSERT INTO compat_identity_rows (label) VALUES ('generated-second') RETURNING id",
                &[],
            )
            .await
            .expect("continue the identity sequence after an explicit value")
            .try_get(0)
            .expect("identity value");
        client
            .batch_execute("ALTER TABLE compat_identity_rows ALTER COLUMN id SET GENERATED ALWAYS")
            .await
            .expect("change identity mode to ALWAYS");
        let rejected_explicit = client
            .query_one(
                "INSERT INTO compat_identity_rows (id, label) VALUES (50, 'rejected') RETURNING id",
                &[],
            )
            .await
            .expect_err("ALWAYS identity should reject an explicit value by default");
        let overridden: i64 = client
            .query_one(
                "INSERT INTO compat_identity_rows (id, label) OVERRIDING SYSTEM VALUE \
                 VALUES (50, 'overridden') RETURNING id",
                &[],
            )
            .await
            .expect("accept explicit identity with OVERRIDING SYSTEM VALUE")
            .try_get(0)
            .expect("overridden identity value");
        let third_generated: i64 = client
            .query_one(
                "INSERT INTO compat_identity_rows (label) VALUES ('generated-third') RETURNING id",
                &[],
            )
            .await
            .expect("continue sequence after an overridden explicit value")
            .try_get(0)
            .expect("identity value");

        // Assert
        assert_eq!(first_generated, 10);
        assert_eq!(explicit, 40);
        assert_eq!(second_generated, 12);
        assert_eq!(db_error(&rejected_explicit).code().code(), "428C9");
        assert_eq!(overridden, 50);
        assert_eq!(third_generated, 14);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_query_results_across_index_maintenance() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("index_maintenance").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_index_rows (id INT PRIMARY KEY, score INT)",
                "INSERT INTO compat_index_rows VALUES (1, 20), (2, 10), (3, 30)",
            ],
        )
        .await
        .expect("create index table");

        // Act
        client
            .batch_execute(
                "CREATE INDEX CONCURRENTLY compat_index_score_idx \
                   ON compat_index_rows (score DESC)",
            )
            .await
            .expect("create ordered index concurrently");
        client
            .batch_execute("REINDEX INDEX compat_index_score_idx")
            .await
            .expect("rebuild index");
        let index_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE tablename = 'compat_index_rows' \
                   AND indexname = 'compat_index_score_idx'",
                &[],
            )
            .await
            .expect("inspect created index")
            .try_get(0)
            .expect("index count");
        let ordered = client
            .query(
                "SELECT score FROM compat_index_rows ORDER BY score DESC",
                &[],
            )
            .await
            .expect("read table after index maintenance");
        client
            .batch_execute("DROP INDEX compat_index_score_idx")
            .await
            .expect("drop an index without a table-specific extension");
        let after_drop: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_indexes \
                 WHERE tablename = 'compat_index_rows' \
                   AND indexname = 'compat_index_score_idx'",
                &[],
            )
            .await
            .expect("inspect dropped index state")
            .try_get(0)
            .expect("remaining index count");

        // Assert
        assert_eq!(index_count, 1);
        assert_eq!(after_drop, 0);
        assert_eq!(
            ordered
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![30, 20, 10]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_route_inserts_to_the_matching_list_partition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("list_partition_routing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_partitioned_rows (id INT, region TEXT) PARTITION BY LIST (region)",
                "CREATE TABLE compat_partitioned_north PARTITION OF compat_partitioned_rows \
                   FOR VALUES IN ('north')",
                "INSERT INTO compat_partitioned_rows VALUES (1, 'north')",
            ],
        )
            .await
            .expect("create partition and inheritance fixtures");

        // Act
        let partition_rows = client
            .query(
                "SELECT id FROM compat_partitioned_rows ORDER BY id",
                &[],
            )
            .await
            .expect("scan partitioned parent");
        let child_rows = client
            .query(
                "SELECT id FROM compat_partitioned_north ORDER BY id",
                &[],
            )
            .await
            .expect("scan the destination partition");

        // Assert
        assert_eq!(partition_rows.len(), 1);
        assert_eq!(partition_rows[0].get::<_, i32>(0), 1);
        assert_eq!(child_rows.len(), 1);
        assert_eq!(child_rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_respect_only_parent_during_inheritance_scans() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("table_inheritance_scans").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_inherited_parent (id INT, label TEXT)",
                "CREATE TABLE compat_inherited_child (child_note TEXT) \
                   INHERITS (compat_inherited_parent)",
                "INSERT INTO compat_inherited_parent VALUES (10, 'parent')",
                "INSERT INTO compat_inherited_child VALUES (11, 'child', 'detail')",
            ],
        )
        .await
        .expect("create parent and inherited child fixtures");

        // Act
        let inherited_rows = client
            .query("SELECT id FROM compat_inherited_parent ORDER BY id", &[])
            .await
            .expect("scan parent and inherited descendants");
        let only_parent = client
            .query(
                "SELECT id FROM ONLY compat_inherited_parent ORDER BY id",
                &[],
            )
            .await
            .expect("scan only the inheritance parent");
        let inherited_catalog = client
            .query(
                "SELECT child.relname FROM pg_catalog.pg_inherits AS i \
                 JOIN pg_catalog.pg_class AS parent ON parent.oid = i.inhparent \
                 JOIN pg_catalog.pg_class AS child ON child.oid = i.inhrelid \
                 WHERE parent.relname = 'compat_inherited_parent'",
                &[],
            )
            .await
            .expect("inspect inherited relation dependency");

        // Assert
        assert_eq!(
            inherited_rows
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![10, 11]
        );
        assert_eq!(only_parent.len(), 1);
        assert_eq!(only_parent[0].get::<_, i32>(0), 10);
        assert_eq!(inherited_catalog.len(), 1);
        assert_eq!(
            inherited_catalog[0].get::<_, String>(0),
            "compat_inherited_child"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_unique_nulls_not_distinct_index_semantics() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unique_index_nulls_not_distinct").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_nulls_not_distinct (value INT)",
                "CREATE UNIQUE INDEX compat_nulls_not_distinct_idx \
                   ON compat_nulls_not_distinct (value) NULLS NOT DISTINCT",
                "INSERT INTO compat_nulls_not_distinct VALUES (NULL)",
            ],
        )
        .await
        .expect("create the unique index and insert its first NULL key");

        // Act
        let duplicate_null = client
            .execute("INSERT INTO compat_nulls_not_distinct VALUES (NULL)", &[])
            .await
            .expect_err("the index must treat NULL keys as equal");
        let remaining_rows: i64 = client
            .query_one("SELECT count(*) FROM compat_nulls_not_distinct", &[])
            .await
            .expect("count rows after duplicate NULL rejection")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(db_error(&duplicate_null).code().code(), "23505");
        assert_eq!(remaining_rows, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_assign_multiple_columns_from_a_joined_mutation_source() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("update_from_tuple_assignment").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_tuple_target (id INT PRIMARY KEY, amount INT, label TEXT)",
                "CREATE TABLE compat_tuple_source (id INT, amount INT, label TEXT)",
                "INSERT INTO compat_tuple_target VALUES (1, 10, 'old-a'), (2, 20, 'old-b'), (3, 30, 'keep')",
                "INSERT INTO compat_tuple_source VALUES (1, 11, 'new-a'), (2, 22, 'new-b')",
            ],
        )
        .await
        .expect("create joined tuple-assignment fixtures");

        // Act
        let updated = client
            .query(
                "UPDATE compat_tuple_target AS target \
                 SET (amount, label) = (source.amount, source.label) \
                 FROM compat_tuple_source AS source \
                 WHERE target.id = source.id \
                 RETURNING target.id, target.amount, target.label",
                &[],
            )
            .await
            .expect("assign a tuple from the joined source");
        let mut updated_rows = updated
            .iter()
            .map(|row| {
                (
                    row.get::<_, i32>(0),
                    row.get::<_, i32>(1),
                    row.get::<_, String>(2),
                )
            })
            .collect::<Vec<_>>();
        updated_rows.sort();
        let unchanged = client
            .query_one(
                "SELECT amount, label FROM compat_tuple_target WHERE id = 3",
                &[],
            )
            .await
            .expect("read the nonmatching target row");

        // Assert
        assert_eq!(
            updated_rows,
            vec![
                (1, 11, "new-a".to_string()),
                (2, 22, "new-b".to_string()),
            ]
        );
        assert_eq!(unchanged.get::<_, i32>(0), 30);
        assert_eq!(unchanged.get::<_, String>(1), "keep");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_assign_multiple_conflict_columns_from_a_subquery() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conflict_tuple_subquery_assignment").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_conflict_tuple_target (id INT PRIMARY KEY, label TEXT, version INT)",
                "CREATE TABLE compat_conflict_tuple_source (id INT, label TEXT, version INT)",
                "INSERT INTO compat_conflict_tuple_target VALUES (1, 'old', 1)",
                "INSERT INTO compat_conflict_tuple_source VALUES (1, 'subquery', 9)",
            ],
        )
        .await
        .expect("create tuple conflict-assignment fixtures");

        // Act
        let changed = client
            .query(
                "INSERT INTO compat_conflict_tuple_target (id, label, version) \
                 VALUES (1, 'incoming', 2) \
                 ON CONFLICT (id) DO UPDATE SET (label, version) = ( \
                   SELECT source.label, source.version \
                   FROM compat_conflict_tuple_source AS source \
                   WHERE source.id = EXCLUDED.id \
                 ) RETURNING id, label, version",
                &[],
            )
            .await
            .expect("assign an upsert target tuple from a scalar subquery");

        // Assert
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].get::<_, i32>(0), 1);
        assert_eq!(changed[0].get::<_, String>(1), "subquery");
        assert_eq!(changed[0].get::<_, i32>(2), 9);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_apply_default_to_a_conflicting_target_column() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("conflict_default_assignment").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_conflict_default (id INT PRIMARY KEY, label TEXT DEFAULT 'fallback')",
                "INSERT INTO compat_conflict_default VALUES (1, 'old')",
            ],
        )
        .await
        .expect("create conflict DEFAULT fixture");

        // Act
        let changed = client
            .query(
                "INSERT INTO compat_conflict_default (id, label) VALUES (1, 'incoming') \
                 ON CONFLICT (id) DO UPDATE SET label = DEFAULT RETURNING id, label",
                &[],
            )
            .await
            .expect("use DEFAULT in the conflict update assignment");

        // Assert
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].get::<_, i32>(0), 1);
        assert_eq!(changed[0].get::<_, String>(1), "fallback");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_attach_a_non_modifying_cte_to_update() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("cte_attached_to_update").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_cte_update_target (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_cte_update_source (id INT, label TEXT)",
                "INSERT INTO compat_cte_update_target VALUES (1, 'old'), (2, 'keep')",
                "INSERT INTO compat_cte_update_source VALUES (1, 'from-cte')",
            ],
        )
        .await
        .expect("create CTE UPDATE fixtures");

        // Act
        let changed = client
            .query(
                "WITH selected AS (SELECT id, label FROM compat_cte_update_source) \
                 UPDATE compat_cte_update_target AS target \
                 SET label = selected.label \
                 FROM selected WHERE target.id = selected.id \
                 RETURNING target.id, target.label",
                &[],
            )
            .await
            .expect("execute UPDATE with a non-modifying WITH query");

        // Assert
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].get::<_, i32>(0), 1);
        assert_eq!(changed[0].get::<_, String>(1), "from-cte");
        let unchanged: String = client
            .query_one(
                "SELECT label FROM compat_cte_update_target WHERE id = 2",
                &[],
            )
            .await
            .expect("read the untouched target row")
            .get(0);
        assert_eq!(unchanged, "keep");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_infer_partial_expression_index_for_conflict_updates() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_expression_conflict_inference").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_partial_conflicts (id INT PRIMARY KEY, email TEXT, active BOOLEAN, version INT)",
                "CREATE UNIQUE INDEX compat_partial_email_active ON compat_partial_conflicts ((lower(email))) WHERE active",
                "INSERT INTO compat_partial_conflicts VALUES (1, 'A@example.com', TRUE, 1), (2, 'a@example.com', FALSE, 9)",
            ],
        )
        .await
        .expect("create partial expression index and seed active/inactive values");

        // Act
        let changed = client
            .query(
                "INSERT INTO compat_partial_conflicts AS target (id, email, active, version) \
                 VALUES (3, 'a@EXAMPLE.com', TRUE, 2) \
                 ON CONFLICT (lower(email)) WHERE active \
                 DO UPDATE SET version = EXCLUDED.version \
                 WHERE target.version < EXCLUDED.version \
                 RETURNING id, version",
                &[],
            )
            .await
            .expect("infer the partial expression index for the conflict target");
        let unchanged = client
            .query(
                "INSERT INTO compat_partial_conflicts AS target (id, email, active, version) \
                 VALUES (4, 'a@example.com', TRUE, 2) \
                 ON CONFLICT (lower(email)) WHERE active \
                 DO UPDATE SET version = EXCLUDED.version \
                 WHERE target.version < EXCLUDED.version \
                 RETURNING id, version",
                &[],
            )
            .await
            .expect("skip an update rejected by its version predicate");
        let stored = client
            .query(
                "SELECT id, email, active, version FROM compat_partial_conflicts ORDER BY active DESC, id",
                &[],
            )
            .await
            .expect("read rows after expression-index conflict handling");

        // Assert
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].get::<_, i32>(0), 1);
        assert_eq!(changed[0].get::<_, i32>(1), 2);
        assert!(unchanged.is_empty(), "a skipped update returns no row");
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].get::<_, i32>(0), 1);
        assert_eq!(stored[0].get::<_, String>(1), "A@example.com");
        assert!(stored[0].get::<_, bool>(2));
        assert_eq!(stored[0].get::<_, i32>(3), 2);
        assert_eq!(stored[1].get::<_, i32>(0), 2);
        assert!(!stored[1].get::<_, bool>(2));
        assert_eq!(stored[1].get::<_, i32>(3), 9);

        drop(client);
        server.shutdown(connection).await;
    });
}
