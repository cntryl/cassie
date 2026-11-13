use super::{ColumnMeta, EnumColumn};
use crate::types::enum_type::{EnumMember, EnumType};
use crate::types::{DataType, EnumRef};

fn state_code() -> DataType {
    DataType::Enum(EnumRef::new(EnumType {
        oid: 20_001,
        array_oid: 20_002,
        qualified_name: "db.public.state_code".to_string(),
        name: "state_code".to_string(),
        revision: 1,
        members: vec![EnumMember {
            member_id: 1,
            label: "CA".to_string(),
            sort_order: 1.0,
            uncommitted: false,
        }],
    }))
}

#[test]
fn should_describe_an_enum_column_with_the_enum_type_oid_not_text() {
    // Arrange
    let data_type = state_code();

    // Act
    let column = ColumnMeta::from_data_type("s", &data_type);

    // Assert
    assert_eq!(column.type_oid, 20_001);
    assert_eq!(column.typlen, 4);
    assert_eq!(
        column.enum_column(),
        Some(EnumColumn {
            element_oid: 20_001,
            is_array: false
        })
    );
    assert!(column.user_codec());
}

#[test]
fn should_describe_an_enum_array_column_with_the_array_type_oid() {
    // Arrange
    let data_type = DataType::Array(Box::new(state_code()));

    // Act
    let column = ColumnMeta::from_data_type("s", &data_type);

    // Assert
    assert_eq!(column.type_oid, 20_002);
    assert_eq!(column.typlen, -1);
    assert_eq!(
        column.enum_column(),
        Some(EnumColumn {
            element_oid: 20_001,
            is_array: true
        })
    );
}

#[test]
fn should_not_flag_a_text_column_as_an_enum() {
    // Arrange / Act
    let column = ColumnMeta::text("t");

    // Assert
    assert_eq!(column.enum_column(), None);
    assert!(!column.user_codec());
}
