use super::*;
use crate::catalog::CollectionMeta;
use crate::types::DataType;

fn catalog_with_table(storage_id: u64) -> Catalog {
    let catalog = Catalog::new();
    let mut meta = CollectionMeta::new("cassie.public.items", None);
    meta.storage_id = storage_id;
    catalog.register_collection_meta_with_constraints(
        meta,
        vec![
            ("id".to_string(), DataType::Int),
            ("name".to_string(), DataType::Text),
        ],
        Vec::new(),
    );
    catalog
}

#[test]
fn should_derive_the_row_type_fields_from_the_table_columns() {
    // Arrange
    let catalog = catalog_with_table(7);

    // Act
    let descriptor = catalog
        .relation_row_type("cassie.public.items")
        .expect("row type");

    // Assert
    let names = descriptor
        .fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["id", "name"]);
    assert_eq!(descriptor.name, "items");
}

#[test]
fn should_keep_the_row_type_oid_stable_for_one_storage_identity() {
    // Arrange
    let first = catalog_with_table(7);
    let second = catalog_with_table(7);

    // Act
    let left = first
        .relation_row_type("cassie.public.items")
        .expect("left");
    let right = second
        .relation_row_type("cassie.public.items")
        .expect("right");

    // Assert
    assert_eq!(left.oid, right.oid);
    assert_ne!(left.oid, left.array_oid);
}

#[test]
fn should_give_different_tables_different_row_type_oids() {
    // Arrange
    let first = relation_row_type_oid(7, false);

    // Act
    let second = relation_row_type_oid(8, false);

    // Assert
    assert_ne!(first, second);
}

#[test]
fn should_keep_derived_oids_out_of_the_allocator_range() {
    // Arrange
    let oid = relation_row_type_oid(7, false);

    // Act
    let flagged = oid & DERIVED_OID_FLAG;

    // Assert
    assert_eq!(flagged, DERIVED_OID_FLAG);
}

#[test]
fn should_resolve_a_table_name_through_the_search_order() {
    // Arrange
    let catalog = catalog_with_table(7);

    // Act
    let resolved = catalog.resolve_relation_row_type("cassie", &["public"], "items");

    // Assert
    assert!(resolved.is_some());
}

#[test]
fn should_not_resolve_a_missing_table_as_a_row_type() {
    // Arrange
    let catalog = catalog_with_table(7);

    // Act
    let resolved = catalog.resolve_relation_row_type("cassie", &["public"], "nothing");

    // Assert
    assert!(resolved.is_none());
}

#[test]
fn should_not_derive_a_row_type_for_a_table_without_storage_identity() {
    // Arrange
    let catalog = catalog_with_table(0);

    // Act
    let descriptor = catalog.relation_row_type("cassie.public.items");

    // Assert
    assert!(descriptor.is_none());
}
