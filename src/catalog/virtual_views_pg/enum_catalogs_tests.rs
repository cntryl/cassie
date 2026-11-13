use super::{enum_rows, type_rows};
use crate::catalog::{Catalog, EnumTypeMeta, ENUM_TYPE_FORMAT_VERSION};
use crate::types::enum_type::EnumMember;
use crate::types::Value;

fn register(catalog: &Catalog, database: &str, name: &str, object_id: u64) {
    let member = |offset: u64, label: &str, sort_order: f32| EnumMember {
        member_id: object_id + offset,
        label: label.to_string(),
        sort_order,
        uncommitted: false,
    };
    catalog.register_enum_type(EnumTypeMeta {
        format_version: ENUM_TYPE_FORMAT_VERSION,
        database: database.to_string(),
        name: format!("{database}.public.{name}"),
        object_id,
        array_object_id: object_id + 1,
        owner_oid: 10,
        revision: 1,
        members: vec![member(2, "ca", 1.0), member(3, "wa", 1.5)],
    });
}

fn value<'a>(row: &'a [(String, Value)], column: &str) -> &'a Value {
    row.iter()
        .find(|(name, _)| name == column)
        .map(|(_, value)| value)
        .expect("column present")
}

#[test]
fn should_publish_enum_and_array_type_rows_with_enum_typtype() {
    // Arrange
    let catalog = Catalog::new();
    register(&catalog, "cassie", "state_code", 80_000);

    // Act
    let rows = type_rows(&catalog, Some("cassie"));

    // Assert
    assert_eq!(rows.len(), 2);
    assert_eq!(value(&rows[0], "typname"), &Value::String("state_code".into()));
    assert_eq!(value(&rows[0], "typtype"), &Value::String("e".into()));
    assert_eq!(value(&rows[0], "typcategory"), &Value::String("E".into()));
    assert_eq!(value(&rows[0], "typlen"), &Value::Int64(4));
    assert_eq!(value(&rows[0], "typbyval"), &Value::Bool(true));
    assert_eq!(value(&rows[0], "typelem"), &Value::Int64(0));
    assert_eq!(value(&rows[1], "typname"), &Value::String("_state_code".into()));
    assert_eq!(value(&rows[1], "typcategory"), &Value::String("A".into()));
    assert_eq!(value(&rows[1], "typelem"), value(&rows[0], "oid"));
    assert_eq!(value(&rows[0], "typarray"), value(&rows[1], "oid"));
}

#[test]
fn should_publish_pg_enum_rows_for_the_current_database_only() {
    // Arrange
    let catalog = Catalog::new();
    register(&catalog, "cassie", "state_code", 80_000);
    register(&catalog, "other", "mood", 90_000);

    // Act
    let rows = enum_rows(&catalog, Some("cassie"));

    // Assert
    assert_eq!(rows.len(), 2);
    assert_eq!(value(&rows[0], "oid"), &Value::Int64(i64::from(80_002 + crate::types::object_id::FIRST_NORMAL_OBJECT_ID)));
    assert_eq!(value(&rows[0], "enumlabel"), &Value::String("ca".into()));
    assert_eq!(value(&rows[1], "enumsortorder"), &Value::Float64(1.5));
    assert_eq!(value(&rows[0], "enumtypid"), value(&rows[1], "enumtypid"));
}

#[test]
fn should_report_enum_and_enum_array_columns_as_user_defined_udt_names() {
    // Arrange
    let catalog = Catalog::new();
    register(&catalog, "cassie", "state_code", 80_000);
    let descriptor = catalog
        .list_enum_types()
        .first()
        .map(crate::catalog::EnumTypeMeta::descriptor)
        .expect("registered enum");
    let scalar = crate::types::DataType::Enum(descriptor);
    let array = crate::types::DataType::Array(Box::new(scalar.clone()));

    // Act
    let scalar_udt = super::column_udt(&scalar);
    let array_udt = super::column_udt(&array);
    let other = super::column_udt(&crate::types::DataType::Text);

    // Assert
    assert_eq!(
        scalar_udt,
        Some(("public".to_string(), "state_code".to_string()))
    );
    assert_eq!(
        array_udt,
        Some(("public".to_string(), "_state_code".to_string()))
    );
    assert_eq!(other, None);
}
