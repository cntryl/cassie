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
