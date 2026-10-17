//! CTE mode serialization preserves legacy default records.
use super::parse_statement;
use crate::sql::ast::{CommonTableExpression, QueryStatement};

#[test]
fn should_allow_nonrecursive_seed_given_recursive_with_when_cte_has_no_union() {
    // Arrange
    let sql = "WITH RECURSIVE seed AS (SELECT 1) SELECT * FROM seed";
    // Act
    let parsed = parse_statement(sql).expect("ordinary seed in recursive WITH");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    // Assert
    assert!(matches!(
        select.ctes[0].query,
        crate::sql::ast::CteQuery::Simple(_)
    ));
}

#[test]
fn should_preserve_legacy_cte_json_given_default_mode_when_round_tripped() {
    // Arrange
    let sql = "WITH chosen AS (SELECT 7 AS value) SELECT value FROM chosen";
    // Act
    let parsed = parse_statement(sql).expect("default CTE");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("SELECT");
    };
    let json = serde_json::to_value(&select.ctes[0]).expect("serialize");
    let restored: CommonTableExpression =
        serde_json::from_value(json.clone()).expect("legacy roundtrip");
    // Assert
    assert!(json.get("materialization").is_none());
    assert_eq!(serde_json::to_value(restored).unwrap(), json);
}

#[test]
fn should_retain_explicit_cte_modes_given_modifiers_when_round_tripped() {
    // Arrange
    let cases = [
        ("MATERIALIZED", "materialized"),
        ("NOT MATERIALIZED", "not_materialized"),
    ];
    // Act
    let modes = cases.map(|(keyword, expected)| {
        let parsed = parse_statement(&format!(
            "WITH chosen AS {keyword} (SELECT 7 AS value) SELECT value FROM chosen"
        ))
        .expect("CTE modifier");
        let QueryStatement::Select(select) = parsed.statement else {
            panic!("SELECT");
        };
        let json = serde_json::to_value(&select.ctes[0]).unwrap();
        let restored: CommonTableExpression = serde_json::from_value(json.clone()).unwrap();
        (json, serde_json::to_value(restored).unwrap(), expected)
    });
    // Assert
    for (original, restored, expected) in modes {
        assert_eq!(original["materialization"], expected);
        assert_eq!(original, restored);
    }
}
