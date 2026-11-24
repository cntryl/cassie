//! ARRAY casts consume the shared expression constructor grammar.
use super::*;

#[test]
fn should_preserve_element_casts_given_a_text_array_constructor_target() {
    // Arrange
    let cases = [
        "ARRAY['x'::CHAR(3)]::TEXT[]",
        "CAST(ARRAY['x'::CHAR(3)] AS TEXT[])",
    ];

    // Act
    for sql in cases {
        let parsed = parse_expression(sql).expect("general ARRAY expression cast");
        let Expr::Cast { expr, .. } = parsed else {
            panic!("outer ARRAY cast");
        };
        let Expr::Function(function) = *expr else {
            panic!("native ARRAY constructor");
        };

        // Assert
        assert_eq!(function.name, crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION);
        assert!(matches!(
            &function.args[0],
            Expr::Cast {
                data_type: crate::types::DataType::Char { length: Some(3) },
                ..
            }
        ));
    }
}
