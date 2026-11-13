use crate::catalog::enum_types::{EnumTypeMeta, ENUM_TYPE_FORMAT_VERSION};
use crate::catalog::Catalog;
use crate::types::enum_type::EnumMember;
use crate::types::DataType;

fn mood(revision: u64, labels: &[&str]) -> EnumTypeMeta {
    EnumTypeMeta {
        format_version: ENUM_TYPE_FORMAT_VERSION,
        database: "cassie".to_owned(),
        name: "cassie.public.mood".to_owned(),
        object_id: 30_000,
        array_object_id: 30_001,
        owner_oid: 10,
        revision,
        members: labels
            .iter()
            .enumerate()
            .map(|(position, label)| EnumMember {
                member_id: 31_000 + u64::try_from(position).expect("small position"),
                label: (*label).to_owned(),
                sort_order: crate::types::enum_type::sort_order_at(position),
                uncommitted: false,
            })
            .collect(),
    }
}

fn catalog_with_stale_columns() -> (Catalog, EnumTypeMeta) {
    let catalog = Catalog::new();
    let stale = mood(1, &["sad"]).descriptor();
    catalog.register_collection(
        "cassie.public.people",
        vec![
            ("a".to_owned(), DataType::Enum(stale.clone())),
            (
                "b".to_owned(),
                DataType::Array(Box::new(DataType::Enum(stale))),
            ),
            ("c".to_owned(), DataType::Text),
        ],
    );
    let current = mood(2, &["sad", "happy"]);
    catalog.register_enum_type(current.clone());
    (catalog, current)
}

#[test]
fn should_propagate_a_changed_descriptor_to_every_embedding_column() {
    // Arrange
    let (catalog, current) = catalog_with_stale_columns();

    // Act
    let propagation = catalog.propagate_enum_change(&current.descriptor());

    // Assert
    let columns = propagation
        .columns
        .iter()
        .map(|column| column.column.as_str())
        .collect::<Vec<_>>();
    assert_eq!(columns, ["a", "b"]);
    assert_eq!(
        propagation.columns[0].data_type,
        DataType::Enum(current.descriptor())
    );
}

#[test]
fn should_replace_stale_embedded_descriptors_with_the_registered_type() {
    // Arrange
    let (catalog, current) = catalog_with_stale_columns();

    // Act
    let changed = catalog.reconcile_embedded_enums();

    // Assert
    let schema = catalog.get_schema("cassie.public.people").expect("schema");
    assert_eq!(changed, 2);
    assert_eq!(
        schema.fields[0].data_type,
        DataType::Enum(current.descriptor())
    );
    assert_eq!(
        schema.fields[1].data_type,
        DataType::Array(Box::new(DataType::Enum(current.descriptor())))
    );
}

#[test]
fn should_leave_matching_embedded_descriptors_unchanged() {
    // Arrange
    let (catalog, _) = catalog_with_stale_columns();
    let _healed = catalog.reconcile_embedded_enums();

    // Act
    let changed = catalog.reconcile_embedded_enums();

    // Assert
    assert_eq!(changed, 0);
}

#[test]
fn should_not_touch_columns_of_other_enum_types() {
    // Arrange
    let catalog = Catalog::new();
    let mut other = mood(1, &["a"]);
    other.name = "cassie.public.other".to_owned();
    other.object_id = 40_000;
    other.array_object_id = 40_001;
    catalog.register_collection(
        "cassie.public.t",
        vec![("a".to_owned(), DataType::Enum(other.descriptor()))],
    );
    let target = mood(2, &["a", "b"]);

    // Act
    let propagation = catalog.propagate_enum_change(&target.descriptor());

    // Assert
    assert_eq!(propagation.columns, Vec::new());
    assert!(propagation.views.is_empty());
}
