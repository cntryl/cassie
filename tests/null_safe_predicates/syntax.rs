use cassie::sql::parse_statement;

#[test]
fn should_parse_null_safe_predicates_at_comparison_precedence() {
    // Arrange
    let statements = [
        ("SELECT 1 IS DISTINCT FROM 2 AS different", "IsDistinctFrom"),
        (
            "SELECT NOT 1 + 1 IS /*gap*/ NOT DISTINCT FROM 2 AND true AS same",
            "IsNotDistinctFrom",
        ),
    ];
    // Act
    for (sql, operator) in statements {
        let parsed = parse_statement(sql).expect("selected null-safe grammar");
        let ast = serde_json::to_value(parsed).expect("transient AST serialization");
        // Assert
        assert!(ast.to_string().contains(operator), "{ast}");
    }
}

#[test]
fn should_preserve_null_safe_keyword_literal_bytes() {
    // Arrange
    let sql = "SELECT 'IS NOT DISTINCT FROM' AS literal";
    // Act
    let parsed = parse_statement(sql).expect("ordinary quoted literal");
    let ast = serde_json::to_value(parsed).expect("AST");
    // Assert
    assert!(ast.to_string().contains("IS NOT DISTINCT FROM"));
    assert!(!ast.to_string().contains("IsNotDistinctFrom"));
}

#[test]
fn should_reject_unparenthesized_null_safe_comparison_chains() {
    // Arrange
    let malformed = [
        "SELECT TRUE IS DISTINCT FROM FALSE IS DISTINCT FROM FALSE AS result",
        "SELECT TRUE IS NOT DISTINCT FROM FALSE IS NOT DISTINCT FROM TRUE AS result",
        "SELECT 1 IS DISTINCT DISTINCT FROM 2 AS result",
        "SELECT 1 IS DISTINCT FROM AS result",
    ];
    // Act
    let results = malformed
        .iter()
        .map(|sql| cassie::sql::parse_statement(sql))
        .collect::<Vec<_>>();
    // Assert
    assert!(results.iter().all(Result::is_err), "{results:?}");
    assert!(cassie::sql::parse_statement(
        "SELECT TRUE IS DISTINCT FROM (FALSE IS DISTINCT FROM FALSE) AS result"
    )
    .is_ok());
}

#[test]
fn should_roundtrip_transient_null_safe_ast_variants() {
    // Arrange
    let queries = [
        "SELECT NULL IS DISTINCT FROM $1 AS different",
        "SELECT $1 IS NOT DISTINCT FROM NULL AS same",
    ];
    // Act
    for sql in queries {
        let original = serde_json::to_value(parse_statement(sql).expect("transient parsed AST"))
            .expect("encode transient AST");
        let decoded: cassie::sql::ast::ParsedStatement =
            serde_json::from_value(original.clone()).expect("decode transient AST");
        // Assert
        assert_eq!(
            serde_json::to_value(decoded).expect("roundtrip AST"),
            original
        );
    }
}
