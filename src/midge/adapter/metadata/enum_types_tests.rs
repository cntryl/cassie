use super::validate_enum_type;
use crate::app::CassieError;
use crate::catalog::{EnumTypeMeta, ENUM_TYPE_FORMAT_VERSION};
use crate::types::EnumMember;

fn member(member_id: u64, label: &str, sort_order: f32) -> EnumMember {
    EnumMember {
        member_id,
        label: label.to_owned(),
        sort_order,
        uncommitted: false,
    }
}

fn valid() -> EnumTypeMeta {
    EnumTypeMeta {
        format_version: ENUM_TYPE_FORMAT_VERSION,
        database: "db".to_owned(),
        name: "db.public.mood".to_owned(),
        object_id: 5,
        array_object_id: 6,
        owner_oid: 10,
        revision: 1,
        members: vec![member(7, "sad", 1.0), member(8, "happy", 2.0)],
    }
}

#[test]
fn should_accept_a_well_formed_descriptor() {
    // Arrange
    let metadata = valid();

    // Act
    let result = validate_enum_type(&metadata);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn should_accept_an_enum_without_members() {
    // Arrange
    let mut metadata = valid();
    metadata.members.clear();

    // Act
    let result = validate_enum_type(&metadata);

    // Assert
    assert!(result.is_ok());
}

#[test]
fn should_reject_an_unknown_format_version() {
    // Arrange
    let mut metadata = valid();
    metadata.format_version = ENUM_TYPE_FORMAT_VERSION + 1;

    // Act
    let result = validate_enum_type(&metadata);

    // Assert
    assert!(matches!(result, Err(CassieError::Unsupported(_))));
}

#[test]
fn should_reject_duplicate_type_and_array_identities() {
    // Arrange
    let mut metadata = valid();
    metadata.array_object_id = metadata.object_id;

    // Act
    let result = validate_enum_type(&metadata);

    // Assert
    assert!(matches!(result, Err(CassieError::Unsupported(_))));
}

#[test]
fn should_reject_duplicate_member_identities_and_labels() {
    // Arrange
    let mut same_id = valid();
    same_id.members[1].member_id = 7;
    let mut same_label = valid();
    same_label.members[1].label = "sad".to_owned();

    // Act
    let by_id = validate_enum_type(&same_id);
    let by_label = validate_enum_type(&same_label);

    // Assert
    assert!(matches!(by_id, Err(CassieError::Unsupported(_))));
    assert!(matches!(by_label, Err(CassieError::Unsupported(_))));
}

#[test]
fn should_reject_members_whose_sort_order_does_not_increase() {
    // Arrange
    let mut metadata = valid();
    metadata.members[1].sort_order = 1.0;

    // Act
    let result = validate_enum_type(&metadata);

    // Assert
    assert!(matches!(result, Err(CassieError::Unsupported(_))));
}

#[test]
fn should_not_persist_the_uncommitted_flag() {
    // Arrange
    let mut metadata = valid();
    metadata.members[0].uncommitted = true;

    // Act
    let json = serde_json::to_string(&metadata).expect("serialize");
    let restored: EnumTypeMeta = serde_json::from_str(&json).expect("deserialize");

    // Assert
    assert!(!json.contains("uncommitted"));
    assert!(!restored.members[0].uncommitted);
}
