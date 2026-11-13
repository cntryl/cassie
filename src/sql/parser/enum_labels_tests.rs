use super::{
    parse_add_value, parse_label_list, parse_rename_value, strip_enum_keyword, take_label,
};
use crate::sql::SqlErrorKind;
use crate::types::EnumValuePlacement;

#[test]
fn should_decode_doubled_quotes_in_a_label() {
    // Arrange
    let text = "'it''s' rest";

    // Act
    let (label, rest) = take_label(text).expect("label");

    // Assert
    assert_eq!(label, "it's");
    assert_eq!(rest, "rest");
}

#[test]
fn should_reject_an_unterminated_label() {
    // Arrange
    let text = "'open";

    // Act
    let error = take_label(text).expect_err("unterminated");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Syntax);
}

#[test]
fn should_parse_labels_containing_list_punctuation() {
    // Arrange
    let inner = "'a,b', ')' , ''";

    // Act
    let labels = parse_label_list(inner).expect("labels");

    // Assert
    assert_eq!(
        labels,
        vec!["a,b".to_owned(), ")".to_owned(), String::new()]
    );
}

#[test]
fn should_parse_an_empty_label_list() {
    // Arrange
    let inner = "  ";

    // Act
    let labels = parse_label_list(inner).expect("labels");

    // Assert
    assert_eq!(labels, Vec::<String>::new());
}

#[test]
fn should_reject_a_trailing_comma_in_the_label_list() {
    // Arrange
    let inner = "'a',";

    // Act
    let error = parse_label_list(inner).expect_err("trailing comma");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Syntax);
}

#[test]
fn should_match_the_enum_keyword_only_as_a_whole_word() {
    // Arrange
    let accepted = ["ENUM ('a')", "enum('a')", "Enum"];
    let rejected = ["enumerate", "enums ('a')"];

    // Act
    let accepted_all = accepted
        .iter()
        .all(|body| strip_enum_keyword(body).is_some());
    let rejected_any = rejected
        .iter()
        .any(|body| strip_enum_keyword(body).is_some());

    // Assert
    assert!(accepted_all);
    assert!(!rejected_any);
}

#[test]
fn should_parse_add_value_if_not_exists_after_a_neighbour() {
    // Arrange
    let text = "IF NOT EXISTS 'new' AFTER 'old'";

    // Act
    let parsed = parse_add_value(text).expect("add value");

    // Assert
    assert_eq!(parsed.label, "new");
    assert!(parsed.if_not_exists);
    assert_eq!(
        parsed.placement,
        Some(EnumValuePlacement::After("old".to_owned()))
    );
}

#[test]
fn should_parse_add_value_without_placement() {
    // Arrange
    let text = "'new'";

    // Act
    let parsed = parse_add_value(text).expect("add value");

    // Assert
    assert!(!parsed.if_not_exists);
    assert!(parsed.placement.is_none());
}

#[test]
fn should_reject_add_value_with_an_unknown_placement_keyword() {
    // Arrange
    let text = "'new' BESIDE 'old'";

    // Act
    let error = parse_add_value(text).expect_err("placement");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Syntax);
}

#[test]
fn should_parse_rename_value() {
    // Arrange
    let text = "'old' TO 'new'";

    // Act
    let (from, to) = parse_rename_value(text).expect("rename value");

    // Assert
    assert_eq!((from.as_str(), to.as_str()), ("old", "new"));
}

#[test]
fn should_reject_rename_value_without_to() {
    // Arrange
    let text = "'old' 'new'";

    // Act
    let error = parse_rename_value(text).expect_err("missing TO");

    // Assert
    assert_eq!(error.kind(), SqlErrorKind::Syntax);
}
