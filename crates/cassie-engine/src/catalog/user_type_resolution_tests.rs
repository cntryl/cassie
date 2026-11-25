use super::UserType;
use crate::catalog::{
    Catalog, CompositeTypeMeta, EnumTypeMeta, COMPOSITE_TYPE_FORMAT_VERSION,
    ENUM_TYPE_FORMAT_VERSION,
};
use crate::types::composite::CompositeField;
use crate::types::enum_type::EnumMember;
use crate::types::DataType;

fn enum_meta(name: &str, object_id: u64) -> EnumTypeMeta {
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
            label: "low".to_owned(),
            sort_order: 1.0,
            uncommitted: false,
        }],
    }
}

fn composite_meta(name: &str, object_id: u64) -> CompositeTypeMeta {
    CompositeTypeMeta {
        format_version: COMPOSITE_TYPE_FORMAT_VERSION,
        database: "db".to_owned(),
        name: name.to_owned(),
        object_id,
        array_object_id: object_id + 1,
        relation_object_id: object_id + 2,
        owner_oid: 10,
        revision: 1,
        fields: vec![CompositeField {
            name: "a".to_owned(),
            data_type: DataType::Int,
        }],
        dropped_positions: Vec::new(),
    }
}

/// `z1.dual` is an enum and `z2.dual` a composite type.
fn catalog_with_shadowed_types() -> Catalog {
    let catalog = Catalog::new();
    catalog.register_enum_type(enum_meta("db.z1.dual", 1000));
    catalog.register_composite_type(composite_meta("db.z2.dual", 2000));
    catalog
}

#[test]
fn should_resolve_an_unqualified_type_to_the_first_schema_on_the_search_path() {
    // Arrange
    let catalog = catalog_with_shadowed_types();

    // Act
    let enum_first = catalog.resolve_user_type("db", &["z1", "z2", "public"], "dual");
    let composite_first = catalog.resolve_user_type("db", &["z2", "z1", "public"], "dual");

    // Assert
    assert!(matches!(enum_first, Some(UserType::Enum(_))));
    assert!(matches!(composite_first, Some(UserType::Composite(_))));
}

#[test]
fn should_resolve_a_qualified_type_whatever_the_search_path_holds() {
    // Arrange
    let catalog = catalog_with_shadowed_types();

    // Act
    let qualified = catalog.resolve_user_type("db", &["z1"], "z2.dual");

    // Assert
    assert_eq!(
        qualified.map(|found| found.type_oid()),
        catalog
            .get_composite_type("db.z2.dual")
            .map(|metadata| metadata.type_oid())
    );
}

#[test]
fn should_report_no_type_when_no_schema_on_the_search_path_holds_the_name() {
    // Arrange
    let catalog = catalog_with_shadowed_types();

    // Act
    let missing = catalog.resolve_user_type("db", &["public"], "dual");

    // Assert
    assert!(missing.is_none());
}
