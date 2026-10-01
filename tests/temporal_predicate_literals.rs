#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

mod temporal_predicate_literals {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::types::Value;

    use support::{data_dir, use_local_storage};

    fn query_ids(
        cassie: &Cassie,
        session: &cassie::app::CassieSession,
        predicate: &str,
    ) -> Vec<i64> {
        let result = cassie
            .execute_sql(
                session,
                &format!("SELECT id FROM temporal_literal_probe WHERE {predicate}"),
                vec![],
            )
            .expect("query temporal literal");
        let mut ids = result
            .rows
            .iter()
            .map(|row| match row[0] {
                Value::Int64(id) => id,
                ref value => panic!("expected integer id, got {value:?}"),
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn should_match_temporal_string_predicates_after_index_creation() {
        // Arrange
        use_local_storage();
        let path = data_dir("temporal_string_predicate_literals");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE temporal_literal_probe (id INT, d DATE, t TIME, ts TIMESTAMP)",
                    vec![],
                )
                .expect("create temporal table");
            for row in [
                "(1, '2024-1-1', '12:00:00.000', '2024-01-01 12:00:00')",
                "(2, '2024-01-02', '13:00:00', '2024-01-02 12:00:00')",
                "(3, '2023-12-31', '11:00:00', '2023-12-31 12:00:00')",
                "(4, '2025-01-01', '23:00:00', '2025-01-01 12:00:00')",
            ] {
                cassie
                    .execute_sql(
                        &session,
                        &format!("INSERT INTO temporal_literal_probe VALUES {row}"),
                        vec![],
                    )
                    .expect("insert temporal values");
            }
            let predicates = [
                (
                    "ts = '2024-01-01 12:00:00'",
                    vec![1],
                    "temporal_literal_ts_idx",
                ),
                (
                    "'2024-01-01T13:00:00+01:00' = ts",
                    vec![1],
                    "temporal_literal_ts_idx",
                ),
                (
                    "ts <= '2024-01-01T12:00:00+00:00'",
                    vec![1, 3],
                    "temporal_literal_ts_idx",
                ),
                (
                    "ts BETWEEN '2024-01-01 00:00:00' AND '2024-01-01 23:59:59'",
                    vec![1],
                    "temporal_literal_ts_idx",
                ),
                ("d = '2024-1-1'", vec![1], "temporal_literal_d_idx"),
                ("t = '12:00:00.000'", vec![1], "temporal_literal_t_idx"),
            ];

            // Act
            for (predicate, expected_ids, _) in &predicates {
                // Assert
                assert_eq!(query_ids(&cassie, &session, predicate), *expected_ids, "{predicate}");
            }

            for field in ["d", "t", "ts"] {
                cassie
                    .execute_sql(
                        &session,
                        &format!(
                            "CREATE INDEX temporal_literal_{field}_idx ON temporal_literal_probe USING btree ({field})"
                        ),
                        vec![],
                    )
                    .expect("create temporal scalar index");
            }
            for (predicate, expected_ids, index_name) in &predicates {
                assert_eq!(query_ids(&cassie, &session, predicate), *expected_ids, "{predicate}");

                let explain = cassie
                    .execute_sql(
                        &session,
                        &format!("EXPLAIN SELECT id FROM temporal_literal_probe WHERE {predicate}"),
                        vec![],
                    )
                    .expect("explain temporal index query");
                let Value::String(plan) = &explain.rows[0][0] else {
                    panic!("expected textual plan");
                };
                assert!(plan.contains(index_name), "plan={plan}");
            }

            let _ = std::fs::remove_dir_all(path);
        });
    }
}
