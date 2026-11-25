use super::support_sql::{data_dir, use_local_storage};
use cassie::app::Cassie;
use cassie::types::Value;

#[test]
fn should_preserve_tab_partitions_through_index_lifecycle() {
    // Arrange
    use_local_storage();
    let path = data_dir("time_series_partition_tabs");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
            let session = cassie.create_session("tester", None);
            for index_first in [false, true] {
                let table = if index_first { "partition_write" } else { "partition_rebuild" };
                cassie.execute_sql(&session, &format!("CREATE TABLE {table} (id INT, tenant TEXT, event_at TIMESTAMP)"), vec![]).expect("create table");
                let index_sql = format!("CREATE INDEX {table}_ts ON {table} USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)");
                if index_first {
                    cassie.execute_sql(&session, &index_sql, vec![]).expect("create index before insert");
                }

                // Act
                for (id, tenant) in [(1, "north\tamerica"), (2, "\t"), (3, "north\t\t"), (4, "north")] {
                    cassie.execute_sql(&session, &format!("INSERT INTO {table} (id, tenant, event_at) VALUES ($1, $2, '2026-01-01T00:30:00Z')"), vec![Value::Int64(id), Value::String(tenant.to_string())]).expect("insert partition value");
                }
                if !index_first {
                    cassie.execute_sql(&session, &index_sql, vec![]).expect("build index over tab partitions");
                }
                let result = cassie.execute_sql(&session, &format!("SELECT id FROM {table} WHERE tenant = $1 AND event_at >= '2026-01-01T00:00:00Z' AND event_at < '2026-01-01T01:00:00Z'"), vec![Value::String("north\tamerica".to_string())]).expect("read indexed tab partition");

                // Assert
                assert_eq!(result.rows(), vec![vec![Value::Int64(1)]]);
            }
            drop(session);
            drop(cassie);
            let reopened = Cassie::new_with_data_dir(&path).expect("reopen local storage");
            reopened.startup().expect("hydrate persisted indexes");
            let session = reopened.create_session("tester", None);
            for table in ["partition_write", "partition_rebuild"] {
                let result = reopened.execute_sql(&session, &format!("SELECT id FROM {table} WHERE tenant = $1 AND event_at >= '2026-01-01T00:00:00Z' AND event_at < '2026-01-01T01:00:00Z'"), vec![Value::String("north\tamerica".to_string())]).expect("read tab partition after restart");
                assert_eq!(result.rows(), vec![vec![Value::Int64(1)]]);
            }
        });
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_read_ambiguous_composite_partitions_across_reopen_after_writes() {
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};

    // Arrange
    let path = data_dir("time_series_composite_partition_identity");
    let open = || {
        let mut config = CassieRuntimeConfig::from_env().expect("query configuration");
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie =
            Cassie::new_local_with_data_dir_and_config(&path, config).expect("open local store");
        cassie.startup().expect("hydrate storage");
        cassie
    };
    {
        let cassie = open();
        let session = cassie.create_session("tester", None);
        for table in ["partition_indexed", "partition_scanned"] {
            seed_composite_partitions(&cassie, &session, table);
        }
        cassie.execute_sql(&session, "CREATE INDEX ambiguous_partition_time ON partition_indexed USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = 'tenant,device')", vec![]).expect("publish composite partition index");
        assert_composite_partitions(&cassie, &session, true);

        // Act
        for table in ["partition_indexed", "partition_scanned"] {
            for mutation in [
                format!("UPDATE {table} SET amount = 41 WHERE id = 1"),
                format!("DELETE FROM {table} WHERE id = 8"),
            ] {
                cassie
                    .execute_sql(&session, &mutation, vec![])
                    .expect("mutate partition rows");
            }
        }

        // Assert
        assert_composite_partitions(&cassie, &session, false);
    }
    let reopened = open();
    let session = reopened.create_session("tester", None);
    assert_composite_partitions(&reopened, &session, false);
    drop(session);
    drop(reopened);
    std::fs::remove_dir_all(path).expect("remove local fixture");
}

fn seed_composite_partitions(cassie: &Cassie, session: &cassie::app::CassieSession, table: &str) {
    const FIRST: &str = "a\u{1f}b";
    const SECOND: &str = "b\u{1f}c";
    let rows = [
        (1, Some(FIRST), "c"),
        (2, Some("a"), SECOND),
        (3, None, "c"),
        (4, Some("null"), "c"),
        (5, Some("a"), "bc"),
        (6, Some("ab"), "c"),
        (7, Some("north\twest"), "c"),
        (8, Some(FIRST), "c"),
    ];
    cassie.execute_sql(session, &format!("CREATE TABLE {table} (id INT PRIMARY KEY, tenant TEXT, device TEXT, event_at TIMESTAMP, amount INT)"), vec![]).expect("create event table");
    for (id, tenant, device) in rows {
        cassie
            .execute_sql(
                session,
                &format!("INSERT INTO {table} VALUES ($1, $2, $3, '2026-01-01T00:30:00Z', $1)"),
                vec![
                    Value::Int64(id),
                    tenant.map_or(Value::Null, Value::from),
                    Value::from(device),
                ],
            )
            .expect("store partition member");
    }
}

fn assert_composite_partitions(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    initial: bool,
) {
    let cases = [
        (Some("a\u{1f}b"), "c", 1, true),
        (Some("a"), "b\u{1f}c", 2, true),
        (None, "c", 3, true),
        (Some("null"), "c", 4, true),
        (Some("a"), "bc", 5, false),
        (Some("ab"), "c", 6, false),
        (Some("north\twest"), "c", 7, false),
    ];
    for (tenant, device, id, ambiguous) in cases {
        let tenant_filter = if tenant.is_none() {
            "tenant IS NULL"
        } else {
            "tenant = $2"
        };
        let mut params = vec![Value::from(device)];
        if let Some(tenant) = tenant {
            params.push(Value::from(tenant));
        }
        let rows = |table: &str| {
            cassie.execute_sql(session, &format!(
                "SELECT id, amount FROM {table} WHERE device = $1 AND {tenant_filter} \
                 AND event_at >= '2026-01-01T00:00:00Z' AND event_at < '2026-01-01T01:00:00Z' ORDER BY id",
            ), params.clone()).expect("read a single logical partition").rows().to_vec()
        };
        let baseline = rows("partition_scanned");
        let before = cassie.metrics();
        let indexed = rows("partition_indexed");
        let after = cassie.metrics();
        let amount = if id == 1 && !initial { 41 } else { id };
        let mut expected = vec![vec![Value::Int64(id), Value::Int64(amount)]];
        if id == 1 && initial {
            expected.push(vec![Value::Int64(8), Value::Int64(8)]);
        }
        assert_eq!(indexed, expected, "tenant {tenant:?}, device {device:?}");
        assert_eq!(indexed, baseline);
        let delta = |counter: &str| {
            after["time_series"][counter].as_u64().expect("counter")
                - before["time_series"][counter]
                    .as_u64()
                    .expect("prior counter")
        };
        assert_eq!(delta("bucket_native_hits"), 1);
        assert_eq!(delta("fallback_scans"), 0);
        assert_eq!(
            delta("row_point_fetches"),
            if ambiguous {
                if initial {
                    8
                } else {
                    7
                }
            } else {
                1
            }
        );
    }
}
