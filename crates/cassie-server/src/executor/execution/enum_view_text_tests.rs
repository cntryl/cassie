use super::{
    followed_by_cast_to, quote_identifier, quote_literal, replace_ranges, tokens, TokenKind,
};

#[test]
fn should_find_string_literals_with_their_ranges() {
    // Arrange
    let sql = "SELECT Id FROM \"My T\" WHERE e >= 'it''s'";

    // Act
    let found = tokens(sql);
    let literal = found.iter().find(|token| token.kind == TokenKind::Literal);

    // Assert
    assert_eq!(literal.map(|token| token.text.as_str()), Some("it's"));
    assert_eq!(
        literal.map(|token| &sql[token.start..token.end]),
        Some("'it''s'")
    );
    assert!(found.iter().any(|token| token.text == "id"));
    assert!(found.iter().any(|token| token.text == "My T"));
}

#[test]
fn should_start_a_dotted_name_at_its_first_part() {
    // Arrange
    let sql = "SELECT 'a'::app.mood";

    // Act
    let found = tokens(sql);
    let mood = found.iter().find(|token| token.text == "mood");

    // Assert
    assert_eq!(
        mood.map(|token| &sql[token.chain_start..token.end]),
        Some("app.mood")
    );
}

#[test]
fn should_recognise_both_cast_spellings_after_a_literal() {
    // Arrange
    let operator = "SELECT 'b' :: app.mood";
    let function = "SELECT CAST('b' AS mood)";
    let plain = "SELECT 'b' AS mood";

    // Act
    let by_operator = followed_by_cast_to(operator, &tokens(operator), 1, "mood");
    let by_function = followed_by_cast_to(function, &tokens(function), 2, "mood");
    let other_type = followed_by_cast_to(operator, &tokens(operator), 1, "app");
    let alias = followed_by_cast_to(plain, &tokens(plain), 1, "other");

    // Assert
    assert!(by_operator);
    assert!(by_function);
    assert!(!other_type);
    assert!(!alias);
}

#[test]
fn should_replace_ranges_without_shifting_earlier_ones() {
    // Arrange
    let sql = "a 'b' c 'b'";
    let ranges = vec![(2, 5, "'bee'".to_owned()), (8, 11, "'bee'".to_owned())];

    // Act
    let rewritten = replace_ranges(sql, &ranges);

    // Assert
    assert_eq!(rewritten, "a 'bee' c 'bee'");
}

#[test]
fn should_quote_only_what_folding_would_change() {
    // Arrange
    let names = ["mood", "Mood", "my mood", "a\"b"];

    // Act
    let quoted = names.map(quote_identifier);
    let literal = quote_literal("it's");

    // Assert
    assert_eq!(quoted, ["mood", "\"Mood\"", "\"my mood\"", "\"a\"\"b\""]);
    assert_eq!(literal, "'it''s'");
}
