use crate::catalog::object_identity::ObjectScope;
use crate::catalog::{Catalog, EnumTypeMeta, ENUM_TYPE_FORMAT_VERSION};
use crate::sql::binder::BindingContext;
use crate::types::enum_type::EnumMember;
use crate::types::ObjectIdKind;

const DATABASE: &str = "enum_identity_test";

fn register(catalog: &Catalog, schema: &str, name: &str, object_id: u64) {
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

fn context(search_path: &[&str]) -> BindingContext {
    BindingContext::new(
        DATABASE,
        search_path.iter().map(ToString::to_string).collect(),
    )
}

#[test]
fn should_resolve_enum_and_array_regtype_text_to_their_oids() {
    // Arrange
    let catalog = Catalog::new();
    register(&catalog, "public", "state_code", 70_000);
    let context = context(&["public"]);
    let scope = ObjectScope::new(&catalog, &context);
    let meta = catalog
        .list_enum_types()
        .first()
        .map_or(0, |meta| meta.type_oid());

    // Act
    let scalar = scope.resolve(ObjectIdKind::RegType, "state_code");
    let array = scope.resolve(ObjectIdKind::RegType, "public.state_code[]");

    // Assert
    assert_eq!(scalar.ok().flatten(), Some(meta));
    assert!(array.ok().flatten().is_some_and(|oid| oid != meta));
}

#[test]
fn should_qualify_enum_regtype_label_only_outside_the_search_path() {
    // Arrange
    let catalog = Catalog::new();
    register(&catalog, "s", "state_code", 71_000);
    let oid = catalog
        .list_enum_types()
        .first()
        .map_or(0, |meta| meta.type_oid());
    let array_oid = catalog
        .list_enum_types()
        .first()
        .map_or(0, |meta| meta.array_oid());
    let qualified = context(&["public"]);
    let visible = context(&["s", "public"]);

    // Act
    let outside = ObjectScope::new(&catalog, &qualified).label(ObjectIdKind::RegType, oid);
    let inside = ObjectScope::new(&catalog, &visible).label(ObjectIdKind::RegType, oid);
    let inside_array =
        ObjectScope::new(&catalog, &visible).label(ObjectIdKind::RegType, array_oid);

    // Assert
    assert_eq!(outside, "s.state_code");
    assert_eq!(inside, "state_code");
    assert_eq!(inside_array, "state_code[]");
}
