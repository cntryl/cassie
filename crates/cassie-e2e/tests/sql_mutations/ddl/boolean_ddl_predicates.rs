//! Scalar partial indexes retain their existing serialized Expr format.

use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::catalog::canonical_relation_name;
use cassie::sql::ast::Expr;
use cassie::types::Value;

fn index_fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(label, &[]);
    fixture
        .cassie
        .startup()
        .expect("bootstrap canonical catalog");
    for sql in [
        "CREATE TABLE bool_index_source (k TEXT, n INT, txt TEXT, flag BOOLEAN)",
        "INSERT INTO bool_index_source VALUES ('same', 1, 'no', FALSE), ('same', 2, 'no', FALSE)",
        "CREATE TABLE bool_index_empty (k TEXT, n INT, txt TEXT, flag BOOLEAN)",
    ] {
        fixture.execute(sql).expect("metadata fixture setup");
    }
    fixture
}

#[test]
fn should_validate_partial_index_boolean_roots_before_metadata_publication() {
    // Arrange
    let fixture = index_fixture("bool-index-static-predicate");
    let cases = [
        ("bool_index_source", "idx_bool_number", "n"),
        ("bool_index_source", "idx_bool_text", "txt"),
        ("bool_index_empty", "idx_bool_empty_number", "n"),
        (
            "bool_index_empty",
            "idx_bool_empty_text",
            "CAST('no' AS TEXT)",
        ),
        ("bool_index_empty", "idx_bool_empty_invalid", "'o'"),
    ];

    // Act
    let results = cases.map(|(table, name, predicate)| {
        fixture.execute(&format!(
            "CREATE INDEX {name} ON {table} (k) WHERE {predicate}"
        ))
    });

    // Assert
    for ((table, name, predicate), result) in cases.into_iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "index predicate {predicate}: {result:?}"
        );
        assert!(
            fixture
                .cassie
                .catalog
                .get_index(
                    &canonical_relation_name("postgres", "public", table),
                    &canonical_relation_name("postgres", "public", name),
                )
                .is_none(),
            "failed predicate published index {name}"
        );
        assert!(fixture
            .cassie
            .midge
            .get_index(
                &canonical_relation_name("postgres", "public", table),
                &canonical_relation_name("postgres", "public", name),
            )
            .expect("read persisted index metadata")
            .is_none());
    }
}

#[test]
fn should_use_canonical_false_partial_index_predicates_during_backfill() {
    // Arrange
    let fixture = index_fixture("bool-index-unknown-false");
    let table = canonical_relation_name("postgres", "public", "bool_index_source");
    let name = canonical_relation_name("postgres", "public", "idx_bool_false");

    // Act
    let created =
        fixture.execute("CREATE UNIQUE INDEX idx_bool_false ON bool_index_source (k) WHERE 'no'");

    // Assert
    created.expect("duplicate preexisting keys outside FALSE predicate are legal");
    let metadata = fixture
        .cassie
        .catalog
        .get_index(&table, &name)
        .expect("created scalar partial index");
    let predicate: Expr =
        serde_json::from_str(metadata.predicate.as_deref().expect("stored predicate"))
            .expect("existing serialized Expr predicate format");
    assert!(matches!(predicate, Expr::BoolLiteral(false)));
    fixture
        .execute("INSERT INTO bool_index_source VALUES ('same', 3, 'no', FALSE)")
        .expect("ongoing uniqueness checks use the stored FALSE predicate");
    assert_eq!(
        fixture.rows("SELECT n FROM bool_index_source ORDER BY n"),
        vec![
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(3)]
        ]
    );
}
