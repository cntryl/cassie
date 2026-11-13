use super::{array_oid, coerce_declared, EnumParameter};
use crate::catalog::{Catalog, EnumTypeMeta, ENUM_TYPE_FORMAT_VERSION};
use crate::types::enum_type::EnumMember;
use crate::types::{DataType, SqlArray, Value};

const DATABASE: &str = "enum_param_test";

fn register(catalog: &Catalog, uncommitted_wa: bool) -> (i32, i32) {
    let member = |id: u64, label: &str, order: f32, uncommitted: bool| EnumMember {
        member_id: id,
        label: label.to_string(),
        sort_order: order,
        uncommitted,
    };
    catalog.register_enum_type(EnumTypeMeta {
        format_version: ENUM_TYPE_FORMAT_VERSION,
        database: DATABASE.to_string(),
        name: format!("{DATABASE}.public.state_code"),
        object_id: 5_000,
        array_object_id: 5_001,
        owner_oid: 10,
        revision: 1,
        members: vec![
            member(5_002, "CA", 1.0, false),
            member(5_003, "WA", 2.0, uncommitted_wa),
        ],
    });
    let meta = catalog.list_enum_types().remove(0);
    (
        i32::try_from(meta.type_oid()).expect("oid"),
        i32::try_from(meta.array_oid()).expect("array oid"),
    )
}

fn label_of(value: &Value) -> Option<String> {
    match value {
        Value::Typed(typed) => typed.as_enum().map(|value| value.label().to_string()),
        _ => None,
    }
}

#[test]
fn should_resolve_enum_and_enum_array_parameter_oids_in_their_database() {
    // Arrange
    let catalog = Catalog::new();
    let (oid, array) = register(&catalog, false);

    // Act
    let scalar =
        crate::sql::binder::parameter_data_type_for_oid_in_database(oid, &catalog, DATABASE);
    let list =
        crate::sql::binder::parameter_data_type_for_oid_in_database(array, &catalog, DATABASE);
    let foreign =
        crate::sql::binder::parameter_data_type_for_oid_in_database(oid, &catalog, "other");

    // Assert
    assert!(matches!(scalar, Some(DataType::Enum(_))));
    assert!(matches!(list, Some(DataType::Array(inner)) if matches!(*inner, DataType::Enum(_))));
    assert!(!matches!(foreign, Some(DataType::Enum(_))));
}

#[test]
fn should_parse_a_label_parameter_into_a_typed_enum_value() {
    // Arrange
    let catalog = Catalog::new();
    let (oid, _) = register(&catalog, false);
    let mut params = vec![Value::String("WA".to_string()), Value::Null];

    // Act
    let outcome = coerce_declared(&catalog, DATABASE, &[oid, oid], &mut params);

    // Assert
    assert!(outcome.is_ok());
    assert_eq!(label_of(&params[0]).as_deref(), Some("WA"));
    assert_eq!(params[1], Value::Null);
}

#[test]
fn should_reject_an_unknown_label_parameter_with_22p02() {
    // Arrange
    let catalog = Catalog::new();
    let (oid, _) = register(&catalog, false);
    let mut params = vec![Value::String("ZZ".to_string())];

    // Act
    let outcome = coerce_declared(&catalog, DATABASE, &[oid], &mut params);

    // Assert
    assert_eq!(outcome.err().map(|error| error.sql_state()), Some("22P02"));
}

#[test]
fn should_reject_an_uncommitted_label_parameter_with_55p04_and_a_hint() {
    // Arrange
    let catalog = Catalog::new();
    let (oid, _) = register(&catalog, true);
    let mut params = vec![Value::String("WA".to_string())];

    // Act
    let outcome = coerce_declared(&catalog, DATABASE, &[oid], &mut params);

    // Assert
    let error = outcome.err().expect("55P04");
    assert_eq!(error.sql_state(), "55P04");
}

#[test]
fn should_parse_array_text_and_binary_labels_into_a_typed_enum_array() {
    // Arrange
    let catalog = Catalog::new();
    let (_, array) = register(&catalog, false);
    let labels = SqlArray::from_elements(vec![
        Value::String("CA".to_string()),
        Value::String("WA".to_string()),
    ])
    .expect("array");
    let mut params = vec![Value::String("{CA,WA}".to_string()), Value::Array(labels)];

    // Act
    let outcome = coerce_declared(&catalog, DATABASE, &[array, array], &mut params);

    // Assert
    assert!(outcome.is_ok());
    for param in &params {
        let Value::Array(parsed) = param else {
            panic!("expected an array parameter");
        };
        let rendered: Vec<_> = parsed.elements().iter().filter_map(label_of).collect();
        assert_eq!(rendered, ["CA", "WA"]);
        assert_eq!(
            array_oid(parsed),
            Some(array),
            "typed arrays report the enum array OID"
        );
    }
}

#[test]
fn should_leave_parameters_of_other_declared_types_untouched() {
    // Arrange
    let catalog = Catalog::new();
    let _ = register(&catalog, false);
    let mut params = vec![Value::String("CA".to_string())];

    // Act
    let outcome = coerce_declared(&catalog, DATABASE, &[25], &mut params);

    // Assert
    assert!(outcome.is_ok());
    assert_eq!(params[0], Value::String("CA".to_string()));
    assert!(EnumParameter::for_oid(&catalog, DATABASE, 25).is_none());
}
