use super::*;
use crate::catalog::IndexMeta;
use crate::config::CassieRuntimeConfig;
use crate::midge::StorageFamily;

#[test]
fn should_preserve_cascade_dependents_when_index_expression_metadata_is_malformed() {
    // Arrange
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("drop_column_metadata", None);
    for sql in [
        "CREATE TABLE drop_column_metadata_parent \
         (id INT PRIMARY KEY, obsolete INT, retained INT)",
        "CREATE TABLE drop_column_metadata_child \
         (parent_id INT REFERENCES drop_column_metadata_parent(id))",
        "INSERT INTO drop_column_metadata_parent VALUES (1, 7, 42)",
        "INSERT INTO drop_column_metadata_child VALUES (1)",
        "CREATE VIEW drop_column_metadata_view \
         AS SELECT id FROM drop_column_metadata_parent",
        "CREATE INDEX drop_column_metadata_idx \
         ON drop_column_metadata_parent ((obsolete + 1))",
    ] {
        cassie
            .execute_sql(&session, sql, Vec::new())
            .unwrap_or_else(|error| panic!("execute fixture SQL {sql}: {error}"));
    }
    let table = "drop_column_metadata_parent";
    let index_name = "drop_column_metadata_idx";
    let mut metadata = cassie
        .catalog
        .list_indexes(table)
        .into_iter()
        .find(|index| index.name.ends_with(index_name))
        .expect("expression index metadata");
    metadata.expressions = vec!["malformed expression JSON".to_owned()];
    let index_key = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .expect("scan index metadata")
        .into_iter()
        .find_map(|(key, value)| {
            serde_json::from_slice::<IndexMeta>(&value)
                .ok()
                .filter(|index| index.name.ends_with(index_name))
                .map(|_| key)
        })
        .expect("stored index key");
    let malformed_metadata = serde_json::to_vec(&metadata).expect("malformed index metadata");
    cassie
        .midge
        .raw_put(StorageFamily::Data, &index_key, &malformed_metadata)
        .expect("replace index metadata with malformed expression");

    // Act
    let result = cassie.execute_sql(
        &session,
        "ALTER TABLE drop_column_metadata_parent DROP COLUMN id CASCADE",
        Vec::new(),
    );
    let stored_metadata = cassie
        .midge
        .raw_get(StorageFamily::Data, &index_key)
        .expect("read stored index metadata");
    let surviving_view = cassie
        .execute_sql(
            &session,
            "SELECT id FROM drop_column_metadata_view",
            Vec::new(),
        )
        .expect("read dependent view after failed drop");
    let foreign_key_insert = cassie.execute_sql(
        &session,
        "INSERT INTO drop_column_metadata_child VALUES (99)",
        Vec::new(),
    );

    // Assert
    assert!(
        result.is_err(),
        "malformed scalar index metadata must abort DROP"
    );
    let schema = cassie
        .catalog
        .get_schema(table)
        .expect("original table schema");
    assert!(schema.fields.iter().any(|field| field.name == "id"));
    assert!(
        cassie
            .catalog
            .get_view("drop_column_metadata_view")
            .is_some()
    );
    assert!(
        cassie
            .catalog
            .get_constraints("drop_column_metadata_child")
            .iter()
            .any(|constraint| constraint.foreign_key_name.is_some())
    );
    assert!(
        cassie
            .catalog
            .list_indexes(table)
            .iter()
            .any(|index| index.name.ends_with(index_name))
    );
    assert_eq!(
        stored_metadata.as_deref(),
        Some(malformed_metadata.as_slice())
    );
    assert_eq!(surviving_view.rows()[0][0].to_string(), "1");
    assert!(foreign_key_insert.is_err());
}
