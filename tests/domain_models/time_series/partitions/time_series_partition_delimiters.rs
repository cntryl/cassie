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
