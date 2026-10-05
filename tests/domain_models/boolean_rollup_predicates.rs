//! Reuses the current version-2 parseable-SQL rollup definition format.

use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::catalog::{canonical_relation_name, RollupMeta, RollupState};
use cassie::sql::ast::{Expr, QueryStatement};
use cassie::types::Value;

fn rollup_fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(label, &[]);
    fixture
        .cassie
        .startup()
        .expect("bootstrap canonical catalog");
    for sql in [
            "CREATE TABLE bool_rollup_source (tenant TEXT, event_at TEXT, n INT, txt TEXT, flag BOOLEAN)",
            "INSERT INTO bool_rollup_source VALUES ('a', '2026-01-01T00:05:00Z', 1, 'no', FALSE), ('a', '2026-01-01T00:15:00Z', 2, 'no', FALSE)",
            "CREATE TABLE bool_rollup_empty (tenant TEXT, event_at TEXT, n INT, txt TEXT, flag BOOLEAN)",
    ] {
        fixture.execute(sql).expect("metadata fixture setup");
    }
    fixture
}

#[test]
fn should_validate_rollup_boolean_roots_before_definition_publication() {
    // Arrange
    let fixture = rollup_fixture("bool-rollup-static-predicate");
    let cases = [
        ("bool_rollup_source", "rollup_bool_number", "n"),
        ("bool_rollup_source", "rollup_bool_text", "txt"),
        ("bool_rollup_empty", "rollup_bool_empty_number", "n"),
        (
            "bool_rollup_empty",
            "rollup_bool_empty_text",
            "CAST('no' AS TEXT)",
        ),
        ("bool_rollup_empty", "rollup_bool_empty_invalid", "'o'"),
    ];

    // Act
    let results = cases.map(|(source, name, predicate)| {
        fixture.execute(&format!(
            "CREATE ROLLUP {name} ON {source} USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total WHERE {predicate}"
        ))
    });

    // Assert
    for ((_, name, predicate), result) in cases.into_iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "rollup predicate {predicate}: {result:?}"
        );
        assert!(
            fixture
                .cassie
                .catalog
                .get_rollup(&canonical_relation_name("postgres", "public", name))
                .is_none(),
            "failed predicate published rollup {name}"
        );
        let canonical_name = canonical_relation_name("postgres", "public", name);
        assert!(fixture
            .cassie
            .midge
            .list_rollups()
            .expect("read persisted rollup metadata")
            .iter()
            .all(|metadata| metadata.name != canonical_name));
    }
}

#[test]
fn should_use_canonical_false_rollup_filters_during_refresh() {
    // Arrange
    let fixture = rollup_fixture("bool-rollup-unknown-false");
    let name = canonical_relation_name("postgres", "public", "rollup_bool_false");

    // Act
    let created = fixture.execute(
        "CREATE ROLLUP rollup_bool_false ON bool_rollup_source USING time_bucket('1 hour', event_at) GROUP BY tenant AGGREGATES COUNT(*) AS total WHERE 'no'",
    );

    // Assert
    created.expect("canonical Boolean input in existing rollup WHERE");
    let metadata = fixture
        .cassie
        .catalog
        .get_rollup(&name)
        .expect("created rollup");
    assert_eq!(metadata.version, RollupMeta::CURRENT_VERSION);
    assert_eq!(metadata.state, RollupState::Ready);
    let filter_sql = metadata.filter_expr.as_deref().expect("stored SQL filter");
    let parsed = cassie::sql::parser::parse_statement(&format!("SELECT 1 WHERE {filter_sql}"))
        .expect("existing parseable-SQL definition contract");
    let QueryStatement::Select(parsed) = parsed.statement else {
        panic!("SELECT wrapper for persisted filter");
    };
    assert!(matches!(parsed.filter, Some(Expr::BoolLiteral(false))));
    let output_name = metadata
        .output_collection
        .rsplit('.')
        .next()
        .expect("rollup local output name");
    assert_eq!(
        fixture.rows(&format!("SELECT COUNT(*) FROM {output_name}")),
        vec![vec![Value::Int64(0)]]
    );
    fixture
        .execute("REFRESH ROLLUP rollup_bool_false")
        .expect("refresh reparses the persisted canonical filter");
    assert_eq!(
        fixture.rows(&format!("SELECT COUNT(*) FROM {output_name}")),
        vec![vec![Value::Int64(0)]]
    );
    assert_eq!(
        fixture.rows("SELECT COUNT(*) FROM bool_rollup_source"),
        vec![vec![Value::Int64(2)]]
    );
}
