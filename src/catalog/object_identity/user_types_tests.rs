use crate::catalog::object_identity::ObjectScope;
use crate::catalog::{
    Catalog, CompositeTypeMeta, EnumTypeMeta, COMPOSITE_TYPE_FORMAT_VERSION,
    ENUM_TYPE_FORMAT_VERSION,
};
use crate::sql::binder::BindingContext;
use crate::types::composite::CompositeField;
use crate::types::enum_type::EnumMember;
use crate::types::{DataType, ObjectIdKind, ObjectIdentifierError};

const DATABASE: &str = "user_type_identity_test";

fn register_enum(catalog: &Catalog, schema: &str, name: &str, object_id: u64) {
    catalog.register_enum_type(EnumTypeMeta {
        format_version: ENUM_TYPE_FORMAT_VERSION,
        database: DATABASE.to_string(),
        name: format!("{DATABASE}.{schema}.{name}"),
        object_id,
        array_object_id: object_id + 1,
        owner_oid: 10,
        revision: 1,
        members: vec![EnumMember {
            member_id: object_id + 2,
            label: "a".to_string(),
            sort_order: 1.0,
            uncommitted: false,
        }],
    });
}

fn register_composite(catalog: &Catalog, schema: &str, name: &str, object_id: u64) {
    catalog.register_composite_type(CompositeTypeMeta {
        format_version: COMPOSITE_TYPE_FORMAT_VERSION,
        database: DATABASE.to_string(),
        name: format!("{DATABASE}.{schema}.{name}"),
        object_id,
        array_object_id: object_id + 1,
        relation_object_id: object_id + 2,
        owner_oid: 10,
        revision: 1,
        fields: vec![CompositeField {
            name: "a".to_string(),
            data_type: DataType::Int,
        }],
        dropped_positions: Vec::new(),
    });
}

fn context(search_path: &[&str]) -> BindingContext {
    BindingContext::new(
        DATABASE,
        search_path.iter().map(ToString::to_string).collect(),
    )
}

/// `z1.dual` is an enum and `z2.dual` a composite type.
fn shadowed_catalog() -> Catalog {
    let catalog = Catalog::new();
    register_enum(&catalog, "z1", "dual", 70_000);
    register_composite(&catalog, "z2", "dual", 71_000);
    catalog
}

fn resolved(scope: &ObjectScope<'_>, text: &str) -> Option<u32> {
    scope.resolve(ObjectIdKind::RegType, text).ok().flatten()
}

#[test]
fn should_resolve_an_unqualified_regtype_to_the_first_schema_on_the_search_path() {
    // Arrange
    let catalog = shadowed_catalog();
    let enum_first = context(&["z1", "z2"]);
    let composite_first = context(&["z2", "z1"]);

    // Act
    let through_enum_schema = resolved(&ObjectScope::new(&catalog, &enum_first), "dual");
    let through_composite_schema = resolved(&ObjectScope::new(&catalog, &composite_first), "dual");

    // Assert
    let scope = ObjectScope::new(&catalog, &enum_first);
    assert_eq!(through_enum_schema, resolved(&scope, "z1.dual"));
    assert_eq!(through_composite_schema, resolved(&scope, "z2.dual"));
    assert_ne!(through_enum_schema, through_composite_schema);
}

#[test]
fn should_qualify_a_regtype_label_shadowed_by_another_kind_of_type() {
    // Arrange
    let catalog = shadowed_catalog();
    let context = context(&["z1", "z2"]);
    let scope = ObjectScope::new(&catalog, &context);
    let enum_oid = resolved(&scope, "z1.dual").unwrap_or(0);
    let composite_oid = resolved(&scope, "z2.dual").unwrap_or(0);

    // Act
    let visible = scope.label(ObjectIdKind::RegType, enum_oid);
    let shadowed = scope.label(ObjectIdKind::RegType, composite_oid);

    // Assert
    assert_eq!(visible, "dual");
    assert_eq!(shadowed, "z2.dual");
    assert_eq!(resolved(&scope, &shadowed), Some(composite_oid));
}

#[test]
fn should_resolve_every_array_spelling_of_a_user_type_to_its_array_type() {
    // Arrange
    let catalog = Catalog::new();
    register_composite(&catalog, "public", "pair", 72_000);
    register_enum(&catalog, "s2", "lvl", 73_000);
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);
    let pair_array = resolved(&scope, "pair[]");
    let lvl_array = resolved(&scope, "s2.lvl[]");

    // Act
    let pair_spellings = ["_pair", "public._pair", "pair[][]", "pair[3]", "pair []"]
        .map(|text| resolved(&scope, text));
    let lvl_underscore = resolved(&scope, "s2._lvl");

    // Assert
    assert!(pair_array.is_some());
    assert_eq!(pair_spellings, [pair_array; 5]);
    assert!(lvl_array.is_some());
    assert_eq!(lvl_underscore, lvl_array);
}

#[test]
fn should_match_a_quoted_type_name_exactly() {
    // Arrange
    let catalog = Catalog::new();
    register_composite(&catalog, "public", "pair", 74_000);
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let folded = resolved(&scope, "PAIR");
    let quoted_exact = resolved(&scope, "\"pair\"");
    let quoted_other_case = resolved(&scope, "\"PAIR\"");

    // Assert
    assert!(folded.is_some());
    assert_eq!(quoted_exact, folded);
    assert_eq!(quoted_other_case, None);
}

#[test]
fn should_reject_a_type_name_with_too_many_dotted_parts() {
    // Arrange
    let catalog = Catalog::new();
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);

    // Act
    let result = scope.resolve(ObjectIdKind::RegType, "a.b.c.d");

    // Assert
    assert!(matches!(
        result,
        Err(ObjectIdentifierError::ImproperQualifiedName(_))
    ));
}
