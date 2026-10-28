use crate::sql::ast::{Expr, IdentifierPath, QuerySource, TableSampleMethod};
use crate::sql::SqlErrorKind;

use super::parse_query_source;

#[test]
fn should_parse_bernoulli_table_sample() {
    // Arrange
    let sql = "public.events TABLESAMPLE BERNOULLI (25)";

    // Act
    let source = parse_query_source(sql).expect("BERNOULLI source parses");

    // Assert
    let expected_relation = QuerySource::Collection(
        IdentifierPath::parse("public.events").expect("qualified relation path parses"),
    );
    assert!(matches!(
        source,
        QuerySource::TableSample {
            source: sampled_source,
            method: TableSampleMethod::Bernoulli,
            rate: Expr::IntegerLiteral(25),
            repeatable: None,
        } if *sampled_source == expected_relation
    ));
}

#[test]
fn should_parse_system_table_sample() {
    // Arrange
    let sql = "public.events TABLESAMPLE SYSTEM (10)";

    // Act
    let source = parse_query_source(sql).expect("SYSTEM source parses");

    // Assert
    let expected_relation = QuerySource::Collection(
        IdentifierPath::parse("public.events").expect("qualified relation path parses"),
    );
    assert!(matches!(
        source,
        QuerySource::TableSample {
            source: sampled_source,
            method: TableSampleMethod::System,
            rate: Expr::IntegerLiteral(10),
            repeatable: None,
        } if *sampled_source == expected_relation
    ));
}

#[test]
fn should_parse_table_sample_rate_expression() {
    // Arrange
    let sql = "events TABLESAMPLE BERNOULLI (10 + $1)";

    // Act
    let source = parse_query_source(sql).expect("sample rate expression parses");

    // Assert
    let QuerySource::TableSample { rate, .. } = source else {
        panic!("source should be TABLESAMPLE")
    };
    assert!(rate.structurally_eq(&Expr::Binary {
        left: Box::new(Expr::IntegerLiteral(10)),
        op: crate::sql::ast::BinaryOp::Add,
        right: Box::new(Expr::Param(0)),
    }));
}

#[test]
fn should_parse_table_sample_repeatable_seed() {
    // Arrange
    let sql = "events TABLESAMPLE BERNOULLI (10) REPEATABLE ($2)";

    // Act
    let source = parse_query_source(sql).expect("repeatable sample parses");

    // Assert
    let QuerySource::TableSample {
        repeatable: Some(seed),
        ..
    } = source
    else {
        panic!("sample should carry a repeatable seed")
    };
    assert!(seed.structurally_eq(&Expr::Param(1)));
}

#[test]
fn should_parse_table_sample_relation_aliases() {
    // Arrange
    let sql = "public.events TABLESAMPLE BERNOULLI (10) AS sampled (event_id, payload)";

    // Act
    let source = parse_query_source(sql).expect("sampled alias parses");

    // Assert
    let QuerySource::Aliased {
        source,
        alias,
        column_aliases,
    } = source
    else {
        panic!("alias should wrap the sampled source")
    };
    assert_eq!(alias, "sampled");
    assert_eq!(column_aliases, ["event_id", "payload"]);
    let QuerySource::TableSample { source, .. } = *source else {
        panic!("alias source should be TABLESAMPLE")
    };
    assert_eq!(
        *source,
        QuerySource::Collection(
            IdentifierPath::parse("public.events").expect("qualified relation path parses")
        )
    );
}

#[test]
fn should_compare_table_sample_sources_structurally() {
    // Arrange
    let same_sample =
        parse_query_source("events TABLESAMPLE SYSTEM (10) REPEATABLE (4)").expect("sample parses");
    let changed_rate = parse_query_source("events TABLESAMPLE SYSTEM (11) REPEATABLE (4)")
        .expect("sample with another rate parses");

    // Act
    let same_sample_is_equal = same_sample == same_sample.clone();
    let different_rate_is_equal = same_sample == changed_rate;

    // Assert
    assert!(same_sample_is_equal);
    assert!(!different_rate_is_equal);
}

#[test]
fn should_reject_malformed_table_sample_clauses() {
    // Arrange
    let malformed = [
        "events TABLESAMPLE (10)",
        "events TABLESAMPLE OTHER (10)",
        "events TABLESAMPLE BERNOULLI",
        "events TABLESAMPLE BERNOULLI ()",
        "events TABLESAMPLE BERNOULLI (10",
        "events TABLESAMPLE BERNOULLI (10) REPEATABLE",
        "events TABLESAMPLE SYSTEM (10) REPEATABLE ()",
        "events TABLESAMPLE SYSTEM (10) REPEATABLE (4",
    ];

    // Act
    let errors = malformed
        .iter()
        .map(|sql| parse_query_source(sql).expect_err("malformed clause is rejected"))
        .collect::<Vec<_>>();

    // Assert
    assert!(errors
        .iter()
        .all(|error| error.kind() == SqlErrorKind::Syntax));
}
