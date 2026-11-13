use super::{data_type_depends_on_enum, EnumTypeMeta, ENUM_TYPE_FORMAT_VERSION};
use crate::catalog::Catalog;
use crate::types::enum_type::EnumMember;
use crate::types::DataType;

fn meta(name: &str, object_id: u64) -> EnumTypeMeta {
    EnumTypeMeta {
        format_version: ENUM_TYPE_FORMAT_VERSION,
        database: "db".to_owned(),
        name: name.to_owned(),
        object_id,
        array_object_id: object_id + 1,
        owner_oid: 10,
        revision: 1,
        members: vec![EnumMember {
            member_id: object_id + 10,
            label: "sad".to_owned(),
            sort_order: 1.0,
            uncommitted: true,
        }],
    }
}

#[test]
fn should_resolve_an_unqualified_name_through_the_search_order() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_enum_type(meta("db.app.mood", 1000));

    // Act
    let found = catalog.resolve_enum_type("db", &["public", "app"], "mood");
    let missing = catalog.resolve_enum_type("db", &["public"], "mood");

    // Assert
    assert_eq!(
        found.map(|metadata| metadata.name),
        Some("db.app.mood".to_owned())
    );
    assert!(missing.is_none());
}

#[test]
fn should_report_array_columns_of_the_type_as_dependents() {
    // Arrange
    let catalog = Catalog::new();
    let metadata = meta("db.public.mood", 1000);
    let descriptor = metadata.descriptor();
    catalog.register_enum_type(metadata.clone());
    catalog.register_collection(
        "db.public.t",
        vec![
            ("a".to_owned(), DataType::Enum(descriptor.clone())),
            (
                "b".to_owned(),
                DataType::Array(Box::new(DataType::Enum(descriptor))),
            ),
            ("c".to_owned(), DataType::Text),
        ],
    );

    // Act
    let details = catalog
        .enum_type_dependents(metadata.type_oid())
        .iter()
        .map(|dependent| dependent.detail("mood"))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(
        details,
        vec![
            "column a of table t depends on type mood".to_owned(),
            "column b of table t depends on type mood[]".to_owned(),
        ]
    );
}

#[test]
fn should_detect_a_dependency_through_an_array_only_for_the_same_oid() {
    // Arrange
    let metadata = meta("db.public.mood", 1000);
    let other = meta("db.public.other", 2000);
    let array = DataType::Array(Box::new(DataType::Enum(metadata.descriptor())));

    // Act
    let same = data_type_depends_on_enum(&array, metadata.type_oid());
    let different = data_type_depends_on_enum(&array, other.type_oid());

    // Assert
    assert!(same);
    assert!(!different);
}

#[test]
fn should_clear_uncommitted_flags_on_the_published_map() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_enum_type(meta("db.public.mood", 1000));

    // Act
    catalog.clear_uncommitted_enum_labels();

    // Assert
    let stored = catalog.get_enum_type("db.public.mood").expect("type");
    assert!(stored.members.iter().all(|member| !member.uncommitted));
}

#[test]
fn should_list_enum_types_declared_in_one_schema() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_enum_type(meta("db.app.mood", 1000));
    catalog.register_enum_type(meta("db.public.color", 2000));

    // Act
    let names = catalog
        .enum_types_in_schema("db", "app")
        .into_iter()
        .map(|metadata| metadata.name)
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(names, vec!["db.app.mood".to_owned()]);
}

#[test]
fn should_reject_a_relation_name_owned_by_an_enum() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_enum_type(meta("db.public.mood", 1000));

    // Act
    let error = catalog
        .reject_domain_relation_name("db.public.mood")
        .expect_err("name collision");
    let free = catalog.reject_domain_relation_name("db.public.other");

    // Assert
    assert_eq!(error.sql_state(), "42710");
    assert!(free.is_ok());
}

#[test]
fn should_drop_every_enum_of_a_database() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_enum_type(meta("db.public.mood", 1000));

    // Act
    catalog.unregister_enum_types_for_database("DB");

    // Assert
    assert!(!catalog.has_enum_types());
}

#[test]
fn should_clear_uncommitted_flags_in_embedded_column_descriptors() {
    // Arrange
    let catalog = Catalog::new();
    let metadata = meta("db.public.mood", 1000);
    catalog.register_enum_type(metadata.clone());
    catalog.register_collection(
        "db.public.t",
        vec![("a".to_owned(), DataType::Enum(metadata.descriptor()))],
    );

    // Act
    catalog.clear_uncommitted_enum_labels();

    // Assert
    let schema = catalog.get_schema("db.public.t").expect("schema");
    let DataType::Enum(embedded) = &schema.fields[0].data_type else {
        panic!("expected an enum column");
    };
    assert!(embedded.members.iter().all(|member| !member.uncommitted));
}
