use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::midge::adapter::{
    legacy_graph_prefixes_for_diagnostics, legacy_graph_sample_keys_for_diagnostics, StorageFamily,
};

fn open(path: &std::path::Path) -> Cassie {
    let cassie = Cassie::new_local_with_data_dir_and_config(path, CassieRuntimeConfig::default())
        .expect("open local storage");
    cassie.startup().expect("start up");
    cassie
}

#[test]
fn should_remove_legacy_graph_entries_on_startup() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-legacy-graph-{}", uuid::Uuid::new_v4()));
    let cassie = open(&path);
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE kept (id BIGINT PRIMARY KEY)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(&session, "INSERT INTO kept VALUES (1)", vec![])
        .expect("insert row");
    for key in legacy_graph_sample_keys_for_diagnostics() {
        cassie
            .midge
            .raw_put(StorageFamily::Data, &key, b"{}")
            .expect("write a legacy entry");
    }
    cassie.shutdown();
    drop(session);
    drop(cassie);

    // Act
    let recovered = open(&path);
    let leftovers = legacy_graph_prefixes_for_diagnostics()
        .iter()
        .map(|prefix| {
            recovered
                .midge
                .raw_scan_prefix(StorageFamily::Data, prefix)
                .expect("scan legacy prefix")
                .len()
        })
        .sum::<usize>();
    let session = recovered.create_session("tester", None);
    let rows = recovered
        .execute_sql(&session, "SELECT id FROM kept", vec![])
        .expect("read the former table")
        .rows()
        .len();
    recovered.shutdown();
    drop(session);
    drop(recovered);
    std::fs::remove_dir_all(path).expect("remove fixture");

    // Assert
    assert_eq!(leftovers, 0);
    assert_eq!(rows, 1);
}
