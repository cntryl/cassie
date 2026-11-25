use super::*;
use crate::catalog::CollectionMeta;
use crate::types::DataType;

fn catalog_with_tables(tables: &[(&str, u64)]) -> Catalog {
    let catalog = Catalog::new();
    for (name, row_type_id) in tables {
        let mut meta = CollectionMeta::new(format!("cassie.public.{name}"), None);
        meta.storage_id = *row_type_id;
        meta.row_type_object_id = *row_type_id + 1;
        meta.row_array_object_id = *row_type_id + 2;
        catalog.register_collection_meta_with_constraints(
            meta,
            vec![
                ("id".to_string(), DataType::Int),
                ("name".to_string(), DataType::Text),
            ],
            Vec::new(),
        );
    }
    catalog
}

fn catalog_with_table(row_type_id: u64) -> Catalog {
    catalog_with_tables(&[("items", row_type_id)])
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
fn should_use_the_persisted_allocator_identities_for_the_row_type() {
    // Arrange
    let catalog = catalog_with_table(7);

    // Act
    let descriptor = catalog
        .relation_row_type("cassie.public.items")
        .expect("row type");

    // Assert
    assert_eq!(
        descriptor.oid,
        crate::catalog::object_identity::normal_oid(8).unwrap()
    );
    assert_eq!(
        descriptor.array_oid,
        crate::catalog::object_identity::normal_oid(9).unwrap()
    );
}

#[test]
fn should_give_different_tables_different_row_type_oids() {
    // Arrange
    let catalog = catalog_with_tables(&[("items", 10), ("orders", 20)]);

    // Act
    let items = catalog
        .relation_row_type("cassie.public.items")
        .expect("items");
    let orders = catalog
        .relation_row_type("cassie.public.orders")
        .expect("orders");

    // Assert
    assert_ne!(items.oid, orders.oid);
}

#[test]
fn should_keep_row_type_oids_out_of_the_hashed_relation_range() {
    // Arrange
    let catalog = catalog_with_table(7);

    // Act
    let descriptor = catalog
        .relation_row_type("cassie.public.items")
        .expect("row type");

    // Assert
    assert!(descriptor.oid < 0x8000_0000);
    assert!(descriptor.array_oid < 0x8000_0000);
}

#[test]
fn should_find_a_row_type_by_oid() {
    // Arrange
    let catalog = catalog_with_table(7);
    let descriptor = catalog
        .relation_row_type("cassie.public.items")
        .expect("row type");

    // Act
    let by_type = catalog.relation_row_type_by_oid(descriptor.oid);

    // Assert
    assert_eq!(by_type.map(|found| found.2), Some(false));
}

#[test]
fn should_find_the_array_row_type_by_oid() {
    // Arrange
    let catalog = catalog_with_table(7);
    let descriptor = catalog
        .relation_row_type("cassie.public.items")
        .expect("row type");

    // Act
    let by_array = catalog.relation_row_type_by_oid(descriptor.array_oid);

    // Assert
    assert_eq!(by_array.map(|found| found.2), Some(true));
}

#[test]
fn should_follow_the_recorded_revision_of_the_row_type() {
    // Arrange
    let catalog = catalog_with_table(7);
    catalog.set_row_type_revision("cassie.public.items", 5);

    // Act
    let descriptor = catalog
        .relation_row_type("cassie.public.items")
        .expect("row type");

    // Assert
    assert_eq!(descriptor.revision, 5);
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
fn should_not_derive_a_row_type_for_a_table_without_allocated_identities() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_collection(
        "cassie.public.items",
        vec![("id".to_string(), DataType::Int)],
    );

    // Act
    let descriptor = catalog.relation_row_type("cassie.public.items");

    // Assert
    assert!(descriptor.is_none());
}

fn register_table(catalog: &Catalog, canonical: &str, storage_id: u64) {
    let mut meta = CollectionMeta::new(canonical.to_string(), None);
    meta.storage_id = storage_id;
    meta.row_type_object_id = storage_id + 1;
    meta.row_array_object_id = storage_id + 2;
    catalog.register_collection_meta_with_constraints(
        meta,
        vec![("id".to_string(), DataType::Int)],
        Vec::new(),
    );
}

#[test]
fn should_list_one_row_type_per_table_when_two_schemas_share_a_table_name() {
    // Arrange
    let catalog = Catalog::new();
    register_table(&catalog, "cassie.public.t1", 10);
    register_table(&catalog, "cassie.s1.t1", 20);

    // Act
    let listed = catalog.list_relation_row_types(Some("cassie"));

    // Assert
    let identities = listed
        .iter()
        .map(|(table, storage_id, descriptor)| (table.as_str(), *storage_id, descriptor.oid))
        .collect::<Vec<_>>();
    assert_eq!(
        identities,
        vec![
            (
                "cassie.public.t1",
                10,
                crate::catalog::object_identity::normal_oid(11).unwrap_or(0)
            ),
            (
                "cassie.s1.t1",
                20,
                crate::catalog::object_identity::normal_oid(21).unwrap_or(0)
            ),
        ]
    );
}

#[test]
fn should_list_only_the_row_types_of_the_requested_database() {
    // Arrange
    let catalog = Catalog::new();
    register_table(&catalog, "cassie.public.rt", 10);
    register_table(&catalog, "otherdb.public.elsewhere", 20);

    // Act
    let current = catalog.list_relation_row_types(Some("cassie"));
    let other = catalog.list_relation_row_types(Some("otherdb"));

    // Assert
    let tables = |listed: &[(String, u64, CompositeRef)]| {
        listed
            .iter()
            .map(|(table, _, _)| table.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(tables(&current), vec!["cassie.public.rt".to_string()]);
    assert_eq!(tables(&other), vec!["otherdb.public.elsewhere".to_string()]);
}
