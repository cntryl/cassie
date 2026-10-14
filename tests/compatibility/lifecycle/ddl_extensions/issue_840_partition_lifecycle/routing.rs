use super::*;

#[test]
fn should_reject_explicit_list_partition_while_default_contains_matching_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue840_default_partition_conflict").await;
        let (client, connection) = server.connect().await;
        let setup = execute_statements(
            &client,
            &[
                "CREATE TABLE compat840_default_root (id INT, region TEXT) \
                   PARTITION BY LIST (region)",
                "CREATE TABLE compat840_default_fallback \
                   PARTITION OF compat840_default_root DEFAULT",
                "INSERT INTO compat840_default_root VALUES (1, 'west'), (2, 'east')",
            ],
        )
        .await;

        // Act
        let create_matching_partition = if setup.is_ok() {
            Some(
                client
                    .batch_execute(
                        "CREATE TABLE compat840_default_west \
                         PARTITION OF compat840_default_root FOR VALUES IN ('west')",
                    )
                    .await,
            )
        } else {
            None
        };
        let default_count_after_rejection = match create_matching_partition.as_ref() {
            Some(Err(_)) => Some(
                client
                    .query_one("SELECT count(*) FROM compat840_default_fallback", &[])
                    .await,
            ),
            Some(Ok(())) | None => None,
        };
        let recovery = match create_matching_partition.as_ref() {
            Some(Err(_)) => {
                let delete_conflicting_row = client
                    .batch_execute("DELETE FROM compat840_default_fallback WHERE region = 'west'")
                    .await;
                let create_partition = if delete_conflicting_row.is_ok() {
                    Some(
                        client
                            .batch_execute(
                                "CREATE TABLE compat840_default_west \
                                 PARTITION OF compat840_default_root \
                                 FOR VALUES IN ('west')",
                            )
                            .await,
                    )
                } else {
                    None
                };
                let insert_after_attach = match create_partition.as_ref() {
                    Some(Ok(())) => Some(
                        client
                            .batch_execute("INSERT INTO compat840_default_root VALUES (3, 'west')")
                            .await,
                    ),
                    Some(Err(_)) | None => None,
                };
                Some((
                    delete_conflicting_row,
                    create_partition,
                    insert_after_attach,
                ))
            }
            Some(Ok(())) | None => None,
        };
        let routed_rows = match recovery.as_ref() {
            Some((_, Some(Ok(())), Some(Ok(())))) => Some(
                client
                    .query(
                        "SELECT id, region FROM compat840_default_root ORDER BY id",
                        &[],
                    )
                    .await,
            ),
            Some(_) | None => None,
        };
        let fallback_rows_after_recovery = match recovery.as_ref() {
            Some((_, Some(Ok(())), Some(Ok(())))) => Some(
                client
                    .query("SELECT id FROM compat840_default_fallback ORDER BY id", &[])
                    .await,
            ),
            Some(_) | None => None,
        };

        // Assert
        let error_state = create_matching_partition
            .as_ref()
            .and_then(|result| result.as_ref().err())
            .map(|error| db_error(error).code().code().to_owned());
        drop(client);
        server.shutdown(connection).await;
        assert!(
            setup.is_ok(),
            "create the list partition and default rows: {setup:?}"
        );
        assert!(
            create_matching_partition
                .as_ref()
                .is_some_and(|result| result.is_err()),
            "a new explicit partition must be rejected while DEFAULT holds a matching row"
        );
        assert_eq!(error_state.as_deref(), Some("23514"));
        assert_eq!(
            default_count_after_rejection
                .expect("count default rows after the rejected partition creation")
                .expect("read the default partition after rejection")
                .get::<_, i64>(0),
            2,
            "rejected partition creation must leave both rows in DEFAULT"
        );
        let (delete_conflicting_row, create_partition, insert_after_attach) = recovery
            .expect("recovery runs only after PostgreSQL rejects the overlapping partition");
        assert!(
            delete_conflicting_row.is_ok(),
            "remove the conflicting default row before creating the partition: \
             {delete_conflicting_row:?}"
        );
        assert!(
            create_partition
                .as_ref()
                .is_some_and(|result| result.is_ok()),
            "the explicit partition should be created after removing its conflicting row"
        );
        assert!(
            insert_after_attach
                .as_ref()
                .is_some_and(|result| result.is_ok()),
            "route a new matching value to the explicit partition after attachment"
        );
        let routed_rows = routed_rows
            .expect("query the parent after the explicit partition accepts a new row")
            .expect("read rows through the partitioned parent");
        assert_eq!(
            routed_rows
                .iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(2, "east".to_owned()), (3, "west".to_owned())]
        );
        let fallback_rows = fallback_rows_after_recovery
            .expect("read the remaining default row after routing")
            .expect("query rows retained by the default partition");
        assert_eq!(fallback_rows.len(), 1);
        assert_eq!(fallback_rows[0].get::<_, i32>(0), 2);
    });
}

#[test]
fn should_validate_existing_rows_before_attaching_a_range_partition() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue840_attach_bound_validation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat840_attach_root (id INT) PARTITION BY RANGE (id)",
                "CREATE TABLE compat840_attach_candidate (id INT)",
                "INSERT INTO compat840_attach_candidate VALUES (3), (12)",
            ],
        )
        .await
        .expect("create an unattached table with one out-of-bound row");

        // Act
        let rejected_attach = client
            .batch_execute(
                "ALTER TABLE compat840_attach_root ATTACH PARTITION \
                 compat840_attach_candidate FOR VALUES FROM (0) TO (10)",
            )
            .await;
        let parent_rows_after_rejection: i64 = client
            .query_one("SELECT count(*) FROM compat840_attach_root", &[])
            .await
            .expect("read the partition parent after rejecting attachment")
            .try_get(0)
            .expect("unattached parent row count");
        let candidate_rows_after_rejection: i64 = client
            .query_one("SELECT count(*) FROM compat840_attach_candidate", &[])
            .await
            .expect("read candidate rows after rejecting attachment")
            .try_get(0)
            .expect("candidate row count");
        let remove_out_of_bound_row = client
            .batch_execute("DELETE FROM compat840_attach_candidate WHERE id = 12")
            .await;
        let accepted_attach = if remove_out_of_bound_row.is_ok() {
            Some(
                client
                    .batch_execute(
                        "ALTER TABLE compat840_attach_root ATTACH PARTITION \
                         compat840_attach_candidate FOR VALUES FROM (0) TO (10)",
                    )
                    .await,
            )
        } else {
            None
        };
        let rows_after_attach = match accepted_attach.as_ref() {
            Some(Ok(())) => Some(
                client
                    .query("SELECT id FROM compat840_attach_root ORDER BY id", &[])
                    .await,
            ),
            Some(Err(_)) | None => None,
        };
        let attachment_count = match accepted_attach.as_ref() {
            Some(Ok(())) => Some(
                client
                    .query_one(
                        "SELECT count(*) FROM pg_catalog.pg_inherits \
                         WHERE inhparent = 'compat840_attach_root'::regclass \
                           AND inhrelid = 'compat840_attach_candidate'::regclass",
                        &[],
                    )
                    .await,
            ),
            Some(Err(_)) | None => None,
        };

        // Assert
        let rejected_state = rejected_attach
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        drop(client);
        server.shutdown(connection).await;
        assert!(
            rejected_attach.is_err(),
            "attachment must reject the candidate while it contains an out-of-bound row"
        );
        assert_eq!(rejected_state.as_deref(), Some("23514"));
        assert_eq!(parent_rows_after_rejection, 0);
        assert_eq!(candidate_rows_after_rejection, 2);
        assert!(
            remove_out_of_bound_row.is_ok(),
            "delete the row that violates the proposed partition bound: \
             {remove_out_of_bound_row:?}"
        );
        assert!(
            accepted_attach
                .as_ref()
                .is_some_and(|result| result.is_ok()),
            "the candidate should attach after its rows satisfy the range bound"
        );
        let rows_after_attach = rows_after_attach
            .expect("read the parent only after attachment succeeds")
            .expect("read the attached candidate through its parent");
        assert_eq!(rows_after_attach.len(), 1);
        assert_eq!(rows_after_attach[0].get::<_, i32>(0), 3);
        assert_eq!(
            attachment_count
                .expect("inspect pg_inherits after attachment")
                .expect("read the parent-child catalog relationship")
                .get::<_, i64>(0),
            1
        );
    });
}

#[test]
fn should_move_updated_rows_between_range_partitions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue840_partition_row_movement").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat840_move_root (id INT, payload TEXT) \
                   PARTITION BY RANGE (id)",
                "CREATE TABLE compat840_move_low PARTITION OF compat840_move_root \
                   FOR VALUES FROM (MINVALUE) TO (10)",
                "CREATE TABLE compat840_move_high PARTITION OF compat840_move_root \
                   FOR VALUES FROM (10) TO (MAXVALUE)",
                "INSERT INTO compat840_move_root VALUES (1, 'move'), (2, 'stay')",
            ],
        )
        .await
        .expect("create range partitions and seed rows on both destinations");

        // Act
        let updated = client
            .query(
                "UPDATE compat840_move_root SET id = 11 WHERE id = 1 \
                 RETURNING id, payload",
                &[],
            )
            .await
            .expect("move the updated row to the other range partition");
        let partition_counts = client
            .query_one(
                "SELECT (SELECT count(*) FROM compat840_move_low), \
                        (SELECT count(*) FROM compat840_move_high)",
                &[],
            )
            .await
            .expect("count rows in both partitions after key movement");
        let parent_rows = client
            .query(
                "SELECT id, payload FROM compat840_move_root ORDER BY id",
                &[],
            )
            .await
            .expect("read rows through the parent after key movement");

        // Assert
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].get::<_, i32>(0), 11);
        assert_eq!(updated[0].get::<_, String>(1), "move");
        assert_eq!(
            (
                partition_counts.get::<_, i64>(0),
                partition_counts.get::<_, i64>(1),
            ),
            (1, 1),
            "the row leaves the low partition and appears in the high partition"
        );
        assert_eq!(
            parent_rows
                .iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
                .collect::<Vec<_>>(),
            vec![(2, "stay".to_owned()), (11, "move".to_owned())]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_route_multicolumn_hash_keys_to_their_remainder_partitions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue840_hash_partition_routing").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat840_hash_root (id INT, region TEXT, label TEXT) \
                   PARTITION BY HASH (id, region)",
                "CREATE TABLE compat840_hash_0 PARTITION OF compat840_hash_root \
                   FOR VALUES WITH (MODULUS 4, REMAINDER 0)",
                "CREATE TABLE compat840_hash_1 PARTITION OF compat840_hash_root \
                   FOR VALUES WITH (MODULUS 4, REMAINDER 1)",
                "CREATE TABLE compat840_hash_2 PARTITION OF compat840_hash_root \
                   FOR VALUES WITH (MODULUS 4, REMAINDER 2)",
                "CREATE TABLE compat840_hash_3 PARTITION OF compat840_hash_root \
                   FOR VALUES WITH (MODULUS 4, REMAINDER 3)",
                "INSERT INTO compat840_hash_root VALUES \
                   (1, 'east', '1e'), (1, 'west', '1w'), \
                   (2, 'east', '2e'), (2, 'west', '2w'), \
                   (3, 'north', '3n'), (4, 'south', '4s'), \
                   (5, 'north', '5n'), (6, 'west', '6w'), \
                   (7, 'east', '7e'), (8, 'south', '8s')",
            ],
        )
        .await
        .expect("create four hash remainders and route multi-column keys");

        // Act
        let routed_rows = client
            .query(
                "SELECT 'compat840_hash_0'::TEXT, id, region \
                   FROM compat840_hash_0 \
                 UNION ALL \
                 SELECT 'compat840_hash_1'::TEXT, id, region \
                   FROM compat840_hash_1 \
                 UNION ALL \
                 SELECT 'compat840_hash_2'::TEXT, id, region \
                   FROM compat840_hash_2 \
                 UNION ALL \
                 SELECT 'compat840_hash_3'::TEXT, id, region \
                   FROM compat840_hash_3 \
                 ORDER BY 1, 2, 3",
                &[],
            )
            .await
            .expect("read the rows stored in each hash remainder partition");

        // Assert
        assert_eq!(
            routed_rows
                .iter()
                .map(|row| (
                    row.get::<_, String>(0),
                    row.get::<_, i32>(1),
                    row.get::<_, String>(2),
                ))
                .collect::<Vec<_>>(),
            vec![
                ("compat840_hash_0".to_owned(), 4, "south".to_owned()),
                ("compat840_hash_1".to_owned(), 1, "west".to_owned()),
                ("compat840_hash_1".to_owned(), 5, "north".to_owned()),
                ("compat840_hash_1".to_owned(), 6, "west".to_owned()),
                ("compat840_hash_2".to_owned(), 1, "east".to_owned()),
                ("compat840_hash_2".to_owned(), 2, "west".to_owned()),
                ("compat840_hash_2".to_owned(), 3, "north".to_owned()),
                ("compat840_hash_2".to_owned(), 7, "east".to_owned()),
                ("compat840_hash_2".to_owned(), 8, "south".to_owned()),
                ("compat840_hash_3".to_owned(), 2, "east".to_owned()),
            ],
            "PostgreSQL routes each multi-column key by its hash remainder"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_route_rows_by_an_expression_partition_key() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue840_expression_partition_key").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat840_expr_root (region TEXT, label TEXT) \
                   PARTITION BY RANGE ((lower(region)))",
                "CREATE TABLE compat840_expr_low PARTITION OF compat840_expr_root \
                   FOR VALUES FROM ('a') TO ('m')",
                "CREATE TABLE compat840_expr_high PARTITION OF compat840_expr_root \
                   FOR VALUES FROM ('m') TO (MAXVALUE)",
                "INSERT INTO compat840_expr_root VALUES \
                   ('Alpha', 'first'), ('Mike', 'boundary'), ('Zulu', 'last')",
            ],
        )
        .await
        .expect("create expression-key range partitions and route mixed-case values");

        // Act
        let routed_rows = client
            .query(
                "SELECT 'compat840_expr_low'::TEXT, region, label \
                   FROM compat840_expr_low \
                 UNION ALL \
                 SELECT 'compat840_expr_high'::TEXT, region, label \
                   FROM compat840_expr_high \
                 ORDER BY 2",
                &[],
            )
            .await
            .expect("read rows stored in each expression-key range partition");

        // Assert
        assert_eq!(
            routed_rows
                .iter()
                .map(|row| (
                    row.get::<_, String>(0),
                    row.get::<_, String>(1),
                    row.get::<_, String>(2),
                ))
                .collect::<Vec<_>>(),
            vec![
                (
                    "compat840_expr_low".to_owned(),
                    "Alpha".to_owned(),
                    "first".to_owned(),
                ),
                (
                    "compat840_expr_high".to_owned(),
                    "Mike".to_owned(),
                    "boundary".to_owned(),
                ),
                (
                    "compat840_expr_high".to_owned(),
                    "Zulu".to_owned(),
                    "last".to_owned(),
                ),
            ],
            "RANGE bounds use lower(region) while preserving the stored input value"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_prune_nonmatching_range_partitions_when_enabled() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue840_partition_pruning_plan").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat840_prune_root (id INT, payload TEXT) \
                   PARTITION BY RANGE (id)",
                "CREATE TABLE compat840_prune_low PARTITION OF compat840_prune_root \
                   FOR VALUES FROM (MINVALUE) TO (10)",
                "CREATE TABLE compat840_prune_mid PARTITION OF compat840_prune_root \
                   FOR VALUES FROM (10) TO (20)",
                "CREATE TABLE compat840_prune_high PARTITION OF compat840_prune_root \
                   FOR VALUES FROM (20) TO (MAXVALUE)",
                "INSERT INTO compat840_prune_root VALUES \
                   (1, 'low'), (12, 'middle'), (25, 'high')",
            ],
        )
        .await
        .expect("create three range partitions with distinguishable bounds");

        // Act
        client
            .batch_execute("SET enable_partition_pruning = on")
            .await
            .expect("enable partition pruning");
        let enabled_plan = client
            .query(
                "EXPLAIN (COSTS OFF) SELECT id FROM compat840_prune_root WHERE id = 12",
                &[],
            )
            .await
            .expect("explain a predicate that selects one range partition");
        client
            .batch_execute("SET enable_partition_pruning = off")
            .await
            .expect("disable partition pruning");
        let disabled_plan = client
            .query(
                "EXPLAIN (COSTS OFF) SELECT id FROM compat840_prune_root WHERE id = 12",
                &[],
            )
            .await
            .expect("explain the same predicate with pruning disabled");
        let enabled_text = enabled_plan
            .iter()
            .map(|row| row.get::<_, String>(0))
            .collect::<Vec<_>>()
            .join("\n");
        let disabled_text = disabled_plan
            .iter()
            .map(|row| row.get::<_, String>(0))
            .collect::<Vec<_>>()
            .join("\n");

        // Assert
        assert!(
            enabled_text.contains("compat840_prune_mid"),
            "{enabled_text}"
        );
        assert!(
            !enabled_text.contains("compat840_prune_low"),
            "{enabled_text}"
        );
        assert!(
            !enabled_text.contains("compat840_prune_high"),
            "{enabled_text}"
        );
        assert!(
            disabled_text.contains("compat840_prune_low"),
            "{disabled_text}"
        );
        assert!(
            disabled_text.contains("compat840_prune_mid"),
            "{disabled_text}"
        );
        assert!(
            disabled_text.contains("compat840_prune_high"),
            "{disabled_text}"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
