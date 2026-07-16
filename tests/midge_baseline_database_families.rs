use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema};
use uuid::Uuid;

fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-layout-v1-{label}-{}", Uuid::new_v4()));
    path.to_string_lossy().into_owned()
}

fn schema() -> Schema {
    Schema {
        fields: vec![FieldSchema {
            name: "value".to_string(),
            data_type: DataType::Text,
            nullable: false,
        }],
    }
}

#[test]
fn should_route_each_database_to_a_stable_opaque_family() {
    // Arrange
    let path = data_dir("routing");
    let first_mapping = {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        seed_duplicate_database_rows(&cassie);
        assert_database_family_layout(&cassie);
        assert_isolated_rows(&cassie);
        database_family_mapping(&cassie)
    };

    // Act
    let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
    restarted.startup().expect("restarted startup");
    let mapping = database_family_mapping(&restarted);

    // Assert
    assert_eq!(mapping, first_mapping);
    assert_isolated_rows(&restarted);
    assert!(restarted
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"doc:")
        .expect("compat scan")
        .is_empty());

    let _ = std::fs::remove_dir_all(path);
}

fn seed_duplicate_database_rows(cassie: &Cassie) {
    cassie
        .midge
        .create_database("analytics", None)
        .expect("create database");
    let primary = canonical_relation_name("postgres", "public", "docs");
    let secondary = canonical_relation_name("analytics", "public", "docs");
    cassie
        .midge
        .create_collection(&primary, schema())
        .expect("primary schema");
    cassie
        .midge
        .create_collection(&secondary, schema())
        .expect("secondary schema");
    cassie
        .midge
        .put_document(
            &primary,
            Some("same-id".to_string()),
            serde_json::json!({"value": "one"}),
        )
        .expect("primary row");
    cassie
        .midge
        .put_document(
            &secondary,
            Some("same-id".to_string()),
            serde_json::json!({"value": "two"}),
        )
        .expect("secondary row");
}

fn database_family_mapping(cassie: &Cassie) -> (String, String) {
    let databases = cassie.midge.list_databases().expect("databases");
    (
        databases
            .iter()
            .find(|entry| entry.name == "postgres")
            .expect("postgres")
            .physical_family
            .clone(),
        databases
            .iter()
            .find(|entry| entry.name == "analytics")
            .expect("analytics")
            .physical_family
            .clone(),
    )
}

fn assert_database_family_layout(cassie: &Cassie) {
    let databases = cassie.midge.list_databases().expect("databases");
    let postgres = databases
        .iter()
        .find(|entry| entry.name == "postgres")
        .expect("postgres");
    let analytics = databases
        .iter()
        .find(|entry| entry.name == "analytics")
        .expect("analytics");
    assert_ne!(postgres.physical_family, analytics.physical_family);
    assert!(!postgres.physical_family.eq_ignore_ascii_case("default"));
    assert!(!postgres.physical_family.eq_ignore_ascii_case("cf1"));
    assert!(!postgres.physical_family.eq_ignore_ascii_case("cf2"));

    for database in ["postgres", "analytics"] {
        let rows = cassie
            .midge
            .raw_scan_prefix_database(database, b"")
            .expect("database data scan");
        assert!(rows.iter().any(|(key, _)| !key
            .windows(database.len())
            .any(|window| window == database.as_bytes())));
    }
}

fn assert_isolated_rows(cassie: &Cassie) {
    for (database, value) in [("postgres", "one"), ("analytics", "two")] {
        let collection = canonical_relation_name(database, "public", "docs");
        assert_eq!(
            cassie
                .midge
                .get_document(&collection, "same-id")
                .expect("row read")
                .expect("row")
                .payload["value"],
            value
        );
    }
}
