//! Exact cost observations call the same private cost routine as runtime selection.

use crate as fixture_cassie;

#[path = "../../../../cassie-e2e/tests/support/join_cost_stats.rs"]
mod fixture;

const STAR: &str = "SELECT a.id FROM a JOIN c ON a.id=c.ak JOIN b ON b.id=c.bk";

fn observed_cost(sql: &str, stats: &fixture::Stats, order: &[&str]) -> u128 {
    let terms =
        super::InnerJoinTerms::from_source(&fixture::plan(sql).source).expect("eligible source");
    let inputs = super::inputs::resolve(&terms.relations, &terms.predicates, stats);
    let indices = order.iter().map(|name| {
        terms.relations.iter().position(|source| {
            matches!(source, crate::sql::ast::QuerySource::Collection(path) if path.as_str() == *name)
        }).expect("order names an input relation")
    }).collect::<Vec<_>>();
    // This is the actual runtime routine, not a second cost oracle.
    super::join_order_cost(&indices, &inputs)
}

#[test]
fn should_observe_the_ndv_cost_of_a_selective_star_order() {
    // Arrange
    let stats = fixture::star_stats(true);

    // Act
    let cost = observed_cost(STAR, &stats, &["a", "c", "b"]);

    // Assert
    assert_eq!(cost, 30);
}

#[test]
fn should_apply_only_one_ready_equality_reduction_per_extension() {
    // Arrange
    let stats = fixture::star_stats(true);
    let sql =
        "SELECT a.id FROM a JOIN c ON a.id=c.ak AND c.ak=a.id JOIN b ON b.id=c.bk AND a.id=b.id";

    // Act
    let cost = observed_cost(sql, &stats, &["a", "c", "b"]);

    // Assert
    assert_eq!(cost, 30);
}

#[test]
fn should_wait_for_all_relations_of_the_owning_on_expression() {
    // Arrange
    let stats = fixture::star_stats(true);
    let sql = "SELECT a.id FROM a JOIN b ON a.id=b.id JOIN c ON b.id=c.bk AND a.id=c.ak";

    // Act
    let cost = observed_cost(sql, &stats, &["a", "c", "b"]);

    // Assert
    assert_eq!(cost, 11_010);
}

#[test]
fn should_keep_known_parent_selectivity_beside_a_correlated_on() {
    // Arrange
    let stats = fixture::correlated_on_stats();

    // Act
    let cost = observed_cost(
        fixture::CORRELATED_ON_SQL,
        &stats,
        &["probe_a_c", "probe_b_a", "probe_c_b"],
    );

    // Assert
    assert_eq!(cost, 6);
}

#[test]
fn should_decline_unsupported_equality_edges() {
    // Arrange
    let stats = fixture::star_stats(true);
    let predicates = [
        "a.id=c.ak OR a.id=c.ak",
        "CAST(a.id AS BIGINT)=c.ak",
        "abs(a.id)=c.ak",
    ];

    // Act
    for predicate in predicates {
        let sql = format!("SELECT a.id FROM a JOIN c ON {predicate} JOIN b ON b.id=c.bk");
        let cost = observed_cost(&sql, &stats, &["a", "c", "b"]);

        // Assert
        assert_eq!(cost, 11_010, "{predicate}");
    }
}

#[test]
fn should_decline_selectivity_for_an_ambiguous_column_qualifier() {
    // Arrange
    let stats = fixture::Stats::from([
        ("s1.doc".to_string(), fixture::collection(10, &[("id", 10)])),
        (
            "s2.doc".to_string(),
            fixture::collection(1_000, &[("id", 1_000)]),
        ),
        ("z".to_string(), fixture::collection(100, &[("id", 100)])),
    ]);
    let sql =
        "SELECT s1.doc.id FROM s1.doc JOIN s2.doc ON doc.id=s2.doc.id JOIN z ON z.id=s2.doc.id";

    // Act
    let cost = observed_cost(sql, &stats, &["s1.doc", "s2.doc", "z"]);

    // Assert
    assert_eq!(cost, 11_010);
}

#[test]
fn should_ceil_a_fitting_u128_product_without_an_overflowing_addition() {
    // Arrange
    let stats = fixture::Stats::from([
        (
            "a".to_string(),
            fixture::collection(u64::MAX, &[("id", 1), ("join_key", 10)]),
        ),
        ("b".to_string(), fixture::collection(274_177, &[("id", 1)])),
        (
            "c".to_string(),
            fixture::collection(67_280_421_310_721, &[("join_key", 10)]),
        ),
    ]);
    let sql = "SELECT a.id FROM a JOIN b ON a.id=b.id JOIN c ON a.join_key=c.join_key";

    // Act
    let cost = observed_cost(sql, &stats, &["a", "b", "c"]);

    // Assert
    // The three row counts multiply to u128::MAX; raw + divisor - 1 overflows.
    assert_eq!(cost, 34_028_236_692_098_904_037_734_102_280_619_518_616);
}

#[test]
fn should_fall_back_for_the_whole_order_when_a_prefix_product_overflows() {
    // Arrange
    let stats = fixture::Stats::from([
        ("a".to_string(), fixture::collection(u64::MAX, &[("id", 1)])),
        ("b".to_string(), fixture::collection(u64::MAX, &[("id", 1)])),
        ("c".to_string(), fixture::collection(u64::MAX, &[("id", 1)])),
    ]);
    let sql = "SELECT a.id FROM a JOIN b ON a.id=b.id JOIN c ON b.id=c.id";

    // Act
    let cost = observed_cost(sql, &stats, &["a", "b", "c"]);

    // Assert
    assert_eq!(cost, u128::MAX);
}

#[test]
fn should_fall_back_for_the_whole_order_when_the_cost_sum_overflows() {
    // Arrange
    let stats = fixture::Stats::from([
        ("a".to_string(), fixture::collection(u64::MAX, &[("id", 1)])),
        ("b".to_string(), fixture::collection(274_177, &[("id", 1)])),
        (
            "c".to_string(),
            fixture::collection(67_280_421_310_721, &[("id", 1)]),
        ),
    ]);
    let sql = "SELECT a.id FROM a JOIN b ON a.id=b.id JOIN c ON b.id=c.id";

    // Act
    let cost = observed_cost(sql, &stats, &["a", "b", "c"]);

    // Assert
    assert_eq!(cost, u128::MAX);
}
