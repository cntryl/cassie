use super::*;
use crate::catalog::{CompositeTypeMeta, COMPOSITE_TYPE_FORMAT_VERSION};
use crate::types::composite::CompositeField;

fn address(revision: u64, fields: &[&str]) -> CompositeTypeMeta {
    CompositeTypeMeta {
        format_version: COMPOSITE_TYPE_FORMAT_VERSION,
        database: "cassie".to_string(),
        name: "cassie.public.address".to_string(),
        object_id: 20_000,
        array_object_id: 20_001,
        relation_object_id: 20_002,
        owner_oid: 10,
        revision,
        fields: fields
            .iter()
            .map(|name| CompositeField {
                name: (*name).to_string(),
                data_type: DataType::Text,
            })
            .collect(),
        dropped_positions: Vec::new(),
    }
}

fn catalog_with_stale_column() -> (Catalog, CompositeTypeMeta) {
    let catalog = Catalog::new();
    let stale = address(1, &["street"]);
    catalog.register_collection(
        "cassie.public.homes",
        vec![("a".to_string(), DataType::Composite(stale.descriptor()))],
    );
    let current = address(2, &["street", "city"]);
    catalog.register_composite_type(current.clone());
    (catalog, current)
}

#[test]
fn should_replace_a_stale_embedded_descriptor_with_the_registered_type() {
    // Arrange
    let (catalog, current) = catalog_with_stale_column();

    // Act
    let changed = catalog.reconcile_embedded_composites();

    // Assert
    let schema = catalog.get_schema("cassie.public.homes").expect("schema");
    assert_eq!(changed, 1);
    assert_eq!(
        schema.fields[0].data_type,
        DataType::Composite(current.descriptor())
    );
}

#[test]
fn should_leave_matching_embedded_descriptors_unchanged() {
    // Arrange
    let (catalog, _) = catalog_with_stale_column();
    catalog.reconcile_embedded_composites();

    // Act
    let changed = catalog.reconcile_embedded_composites();

    // Assert
    assert_eq!(changed, 0);
}

fn table_with_columns(catalog: &Catalog, second: &str) {
    let mut meta = crate::catalog::CollectionMeta::new("cassie.public.t", None);
    meta.storage_id = 70;
    meta.row_type_object_id = 71;
    meta.row_array_object_id = 72;
    catalog.register_collection_meta_with_constraints(
        meta,
        vec![
            ("id".to_string(), DataType::Int),
            (second.to_string(), DataType::Text),
        ],
        Vec::new(),
    );
}

#[test]
fn should_replace_a_stale_embedded_table_row_type_with_the_derived_one() {
    // Arrange
    let before = Catalog::new();
    table_with_columns(&before, "name");
    let stale = before
        .relation_row_type("cassie.public.t")
        .expect("row type");
    let catalog = Catalog::new();
    table_with_columns(&catalog, "label");
    catalog.register_collection(
        "cassie.public.u",
        vec![("x".to_string(), DataType::Composite(stale))],
    );
    let derived = catalog
        .relation_row_type("cassie.public.t")
        .expect("row type");

    // Act
    catalog.reconcile_embedded_composites();

    // Assert
    let schema = catalog.get_schema("cassie.public.u").expect("schema");
    assert_eq!(schema.fields[0].data_type, DataType::Composite(derived));
}
