use super::mentions_enum;
use serde_json::json;

fn cast_to(oid: u32) -> serde_json::Value {
    json!({
        "Cast": {
            "expr": { "StringLiteral": "p" },
            "data_type": {
                "enum": { "oid": oid, "array_oid": oid + 1, "name": "e", "members": [] }
            }
        }
    })
}

#[test]
fn should_find_an_enum_descriptor_nested_in_a_statement() {
    // Arrange
    let statement = json!({ "Select": { "projection": [{ "Expr": { "expr": cast_to(700) } }] } });

    // Act
    let found = mentions_enum(&statement, 700);

    // Assert
    assert!(found);
}

#[test]
fn should_ignore_objects_that_are_not_the_enum_descriptor() {
    // Arrange
    let other_enum = json!({ "Select": { "filter": cast_to(701) } });
    let no_descriptor = json!({ "enum": { "oid": 700 }, "values": [700, "enum"] });

    // Act
    let other = mentions_enum(&other_enum, 700);
    let plain = mentions_enum(&no_descriptor, 700);

    // Assert
    assert!(!other);
    assert!(!plain);
}
