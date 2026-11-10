//! Deferred binder witness for PostgreSQL unknown-literal path inference.
use super::*;

#[test]
fn should_coerce_only_unknown_delete_path_literals_to_text_array() {
    // Arrange
    let mut left = crate::sql::parser::parse_expression("'{}'::JSONB").expect("left");
    let mut op = BinaryOp::JsonbDeletePath;
    let mut literal = Expr::StringLiteral("{a}".into());
    let mut text = crate::sql::parser::parse_expression("'{a}'::TEXT").expect("typed text");
    // Act
    qualify(
        &mut left,
        &mut op,
        &mut literal,
        Some(&DataType::Jsonb),
        None,
    )
    .expect("literal");
    qualify(
        &mut left,
        &mut op,
        &mut text,
        Some(&DataType::Jsonb),
        Some(&DataType::Text),
    )
    .expect("typed text unchanged");
    // Assert
    assert!(
        matches!(literal, Expr::Cast { data_type: DataType::Array(element), .. } if *element == DataType::Text)
    );
    assert!(matches!(
        text,
        Expr::Cast {
            data_type: DataType::Text,
            ..
        }
    ));
}

#[test]
fn should_select_jsonb_for_unknown_left_delete_operand() {
    // Arrange
    let mut left = Expr::StringLiteral(r#"{"a":1}"#.into());
    let mut right = Expr::StringLiteral("{a}".into());
    let mut op = BinaryOp::JsonbDeletePath;
    // Act
    qualify(&mut left, &mut op, &mut right, None, None).expect("unknown operands");
    // Assert
    assert!(matches!(
        left,
        Expr::Cast {
            data_type: DataType::Jsonb,
            ..
        }
    ));
    assert!(
        matches!(right, Expr::Cast { data_type: DataType::Array(element), .. } if *element == DataType::Text)
    );
}
