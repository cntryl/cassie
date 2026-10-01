#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

mod time_series_float_partitions {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::types::Value;

    use support::{data_dir, use_local_storage};

    fn query_ids(
        cassie: &Cassie,
        session: &cassie::app::CassieSession,
        table: &str,
        predicate: &str,
        params: &[Value],
    ) -> Vec<i64> {
        let result = cassie
            .execute_sql(
                session,
                &format!("SELECT id FROM {table} WHERE {predicate}"),
                params.to_vec(),
            )
            .expect("query time-series partition predicate");
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
    fn should_match_full_scan_for_whole_number_float_partition_values() {
        // Arrange
        use_local_storage();
        let path = data_dir("time_series_float_partitions");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            let session = cassie.create_session("tester", None);
            for table in ["float_partition_baseline", "float_partition_indexed"] {
                cassie
                    .execute_sql(
                        &session,
                        &format!(
                            "CREATE TABLE {table} (id INT, ratio FLOAT, tenant TEXT, event_at TIMESTAMP)"
                        ),
                        vec![],
                    )
                    .expect("create time-series table");
                for row in [
                    "(1, 2, 'tenant-a', '2026-01-01T12:00:00Z')",
                    "(2, 2.0, 'tenant-a', '2026-01-01T13:00:00Z')",
                    "(3, 2.5, 'tenant-a', '2026-01-01T14:00:00Z')",
                    "(4, 2, 'tenant-b', '2026-01-01T14:30:00Z')",
                    "(5, -0.0, 'tenant-a', '2026-01-01T14:45:00Z')",
                ] {
                    cassie
                        .execute_sql(
                            &session,
                            &format!("INSERT INTO {table} VALUES {row}"),
                            vec![],
                        )
                        .expect("insert time-series row");
                }
            }
            cassie
                .execute_sql(
                    &session,
                    "CREATE INDEX float_partition_ts_idx ON float_partition_indexed USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = 'ratio,tenant')",
                    vec![],
                )
                .expect("create time-series index");

            let predicates = [
                (
                    "ratio = 2 AND tenant = 'tenant-a' AND event_at >= '2026-01-01T12:00:00Z' AND event_at < '2026-01-01T15:00:00Z'",
                    vec![],
                    vec![1, 2],
                ),
                (
                    "ratio = 2.0 AND tenant = 'tenant-a' AND event_at >= '2026-01-01T12:00:00Z' AND event_at < '2026-01-01T15:00:00Z'",
                    vec![],
                    vec![1, 2],
                ),
                (
                    "ratio = $1 AND tenant = 'tenant-a' AND event_at >= '2026-01-01T12:00:00Z' AND event_at < '2026-01-01T15:00:00Z'",
                    vec![Value::Float64(2.0)],
                    vec![1, 2],
                ),
                (
                    "ratio = 0.0 AND tenant = 'tenant-a' AND event_at >= '2026-01-01T12:00:00Z' AND event_at < '2026-01-01T15:00:00Z'",
                    vec![],
                    vec![5],
                ),
                (
                    "ratio = 0 AND tenant = 'tenant-a' AND event_at >= '2026-01-01T12:00:00Z' AND event_at < '2026-01-01T15:00:00Z'",
                    vec![],
                    vec![5],
                ),
            ];

            // Act
            for (predicate, params, expected) in predicates {
                let baseline = query_ids(
                    &cassie,
                    &session,
                    "float_partition_baseline",
                    predicate,
                    &params,
                );
                let indexed = query_ids(
                    &cassie,
                    &session,
                    "float_partition_indexed",
                    predicate,
                    &params,
                );

                // Assert
                assert_eq!(baseline, expected, "baseline: {predicate}");
                assert_eq!(indexed, baseline, "indexed: {predicate}");
            }

            let _ = std::fs::remove_dir_all(path);
        });
    }
}
