use super::parse_alter_type_statement;
use crate::sql::ast::{QueryStatement, TypeAction, TypeAlteration};
use crate::sql::SqlErrorKind;
use crate::types::DataType;

fn alteration(sql: &str) -> TypeAlteration {
    let parsed = parse_alter_type_statement(sql).expect("parse ALTER TYPE");
    let QueryStatement::Type(statement) = parsed.statement else {
        panic!("expected a type statement");
    };
    let TypeAction::Alter { alteration, .. } = statement.action else {
        panic!("expected ALTER TYPE");
    };
    alteration
}

#[test]
fn should_parse_rename_to() {
    // Arrange
    let sql = "ALTER TYPE addr RENAME TO place";

    // Act
    let parsed = alteration(sql);

    // Assert
    assert!(matches!(parsed, TypeAlteration::RenameTo { new_name } if new_name == "place"));
}

#[test]
fn should_parse_rename_attribute() {
    // Arrange
    let sql = "ALTER TYPE addr RENAME ATTRIBUTE zip TO postcode CASCADE";

    // Act
    let parsed = alteration(sql);

    // Assert
    assert!(matches!(
        parsed,
        TypeAlteration::RenameAttribute { from, to } if from == "zip" && to == "postcode"
    ));
}

#[test]
fn should_parse_add_attribute_with_a_parameterized_type() {
    // Arrange
    let sql = "ALTER TYPE addr ADD ATTRIBUTE price numeric(10, 2) RESTRICT";

    // Act
    let parsed = alteration(sql);

    // Assert
    assert!(matches!(
        parsed,
        TypeAlteration::AddAttribute { attribute } if attribute.name == "price"
    ));
}

#[test]
fn should_parse_drop_attribute_if_exists() {
    // Arrange
    let sql = "ALTER TYPE addr DROP ATTRIBUTE IF EXISTS zip";

    // Act
    let parsed = alteration(sql);

    // Assert
    assert!(matches!(
        parsed,
        TypeAlteration::DropAttribute { name, if_exists } if name == "zip" && if_exists
    ));
}

#[test]
fn should_parse_alter_attribute_set_data_type() {
    // Arrange
    let sql = "ALTER TYPE addr ALTER ATTRIBUTE zip SET DATA TYPE bigint";

    // Act
    let parsed = alteration(sql);

    // Assert
    assert!(matches!(
        parsed,
        TypeAlteration::AlterAttributeType { name, data_type }
            if name == "zip" && data_type == DataType::BigInt
    ));
}

#[test]
fn should_parse_alter_attribute_type_without_set_data() {
    // Arrange
    let sql = "ALTER TYPE addr ALTER ATTRIBUTE zip TYPE text";

    // Act
    let parsed = alteration(sql);

    // Assert
    assert!(matches!(
        parsed,
        TypeAlteration::AlterAttributeType { data_type, .. } if data_type == DataType::Text
    ));
}

#[test]
fn should_parse_set_schema() {
    // Arrange
    let sql = "ALTER TYPE addr SET SCHEMA archive";

    // Act
    let parsed = alteration(sql);

    // Assert
    assert!(matches!(parsed, TypeAlteration::SetSchema { schema } if schema == "archive"));
}

#[test]
fn should_parse_owner_to() {
    // Arrange
    let sql = "ALTER TYPE addr OWNER TO app_owner";

    // Act
    let parsed = alteration(sql);

    // Assert
    assert!(matches!(parsed, TypeAlteration::OwnerTo { owner } if owner == "app_owner"));
}

#[test]
fn should_reject_enum_value_changes_as_unsupported() {
    // Arrange
    let sql = "ALTER TYPE mood ADD VALUE 'ok'";

    // Act
    let error = parse_alter_type_statement(sql).expect_err("enum value");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_several_actions_as_unsupported() {
    // Arrange
    let sql = "ALTER TYPE addr ADD ATTRIBUTE a int, ADD ATTRIBUTE b int";

    // Act
    let error = parse_alter_type_statement(sql).expect_err("several actions");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Unsupported);
}

#[test]
fn should_reject_an_unknown_action_as_a_syntax_error() {
    // Arrange
    let sql = "ALTER TYPE addr FROBNICATE";

    // Act
    let error = parse_alter_type_statement(sql).expect_err("unknown action");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Syntax);
}
