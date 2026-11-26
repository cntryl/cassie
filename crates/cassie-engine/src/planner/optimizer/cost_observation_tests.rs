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

fn reference_order(inputs: &super::inputs::JoinInputs) -> Vec<usize> {
    let mut order = (0..inputs.rows.len()).collect::<Vec<_>>();
    let mut best = None;
    loop {
        let rank = (
            super::join_order_cost(&order, inputs),
            order
                .iter()
                .map(|&index| inputs.labels[index].as_str())
                .collect::<Vec<_>>(),
            order.clone(),
        );
        if best.as_ref().is_none_or(|prior| rank < *prior) {
            best = Some(rank);
        }
        // Generate complete orders independently of the optimizer's DFS.
        let Some(pivot) = order.windows(2).rposition(|pair| pair[0] < pair[1]) else {
            return best.expect("at least one complete order").2;
        };
        let swap = (pivot + 1..order.len())
            .rfind(|&index| order[index] > order[pivot])
            .expect("pivot has a larger successor");
        order.swap(pivot, swap);
        order[pivot + 1..].reverse();
    }
}

#[test]
fn should_rebuild_the_full_order_selected_by_whole_on_costs() {
    // Arrange
    let sql = "SELECT a.id FROM a JOIN b ON a.id=b.id \
               JOIN c ON b.id=c.bk AND a.id=c.ak JOIN d ON c.ak=d.id";
    let mut stats = fixture::star_stats(true);
    stats.insert("d".to_owned(), fixture::collection(5, &[("id", 5)]));
    let plan = fixture::plan(sql);
    let terms = super::InnerJoinTerms::from_source(&plan.source).expect("eligible source");
    let inputs = super::inputs::resolve(&terms.relations, &terms.predicates, &stats);

    // Act
    let reference = terms.rebuild(&reference_order(&inputs), &inputs);
    let optimized = super::optimize_with_stats(plan, &stats);

    // Assert
    assert_eq!(
        serde_json::to_value(optimized.source).expect("serialized optimized source"),
        serde_json::to_value(reference).expect("serialized reference source"),
    );
}

#[test]
fn should_preserve_input_index_order_when_costs_tie_for_duplicate_labels() {
    // Arrange
    let inputs = super::inputs::JoinInputs {
        rows: vec![1; 4],
        labels: ["z", "a", "z", "b"].map(str::to_owned).into(),
        predicates: Vec::new(),
    };

    // Act
    let selected = super::enumerate_join_order(&inputs);

    // Assert
    assert_eq!(selected, reference_order(&inputs));
    assert_eq!(selected, [1, 3, 0, 2]);
}

#[test]
fn should_keep_legacy_fallback_for_complete_orders_with_checked_overflow() {
    // Arrange
    let product = fixture::Stats::from([
        (
            "a".to_owned(),
            fixture::collection(u64::MAX, &[("id", u64::MAX)]),
        ),
        ("b".to_owned(), fixture::collection(u64::MAX, &[("id", 1)])),
        ("c".to_owned(), fixture::collection(2, &[("id", 1)])),
    ]);
    let sum = fixture::Stats::from([
        (
            "a".to_owned(),
            fixture::collection(u64::MAX, &[("id", u64::MAX)]),
        ),
        ("b".to_owned(), fixture::collection(274_177, &[])),
        ("c".to_owned(), fixture::collection(67_280_421_310_721, &[])),
        ("d".to_owned(), fixture::collection(7, &[("id", 7)])),
    ]);
    let cases = [
        (
            "SELECT a.id FROM a JOIN b ON a.id=b.id JOIN c ON a.id=c.id",
            product,
            &[1, 2, 0][..],
        ),
        (
            "SELECT a.id FROM a JOIN b ON TRUE JOIN c ON TRUE JOIN d ON a.id=d.id",
            sum,
            &[0, 1, 2, 3][..],
        ),
    ];

    // Act
    for (sql, stats, overflow_order) in cases {
        let plan = fixture::plan(sql);
        let terms = super::InnerJoinTerms::from_source(&plan.source).expect("eligible source");
        let inputs = super::inputs::resolve(&terms.relations, &terms.predicates, &stats);
        let selected = super::enumerate_join_order(&inputs);

        // Assert
        assert_eq!(super::join_order_cost(overflow_order, &inputs), u128::MAX);
        assert!(super::join_order_cost(&selected, &inputs) < u128::MAX);
        assert_eq!(selected, reference_order(&inputs), "{sql}");
    }
}

std::thread_local! {
    static CHECKED_STEPS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

pub(super) fn record_checked_step() {
    CHECKED_STEPS.with(|steps| {
        if let Some(count) = steps.get() {
            steps.set(Some(count + 1));
        }
    });
}

#[test]
fn should_score_each_exhaustive_join_prefix_once() {
    // Arrange
    let inputs = super::inputs::JoinInputs {
        rows: vec![4; 4],
        labels: ["d", "c", "b", "a"].map(str::to_owned).into(),
        predicates: Vec::new(),
    };
    CHECKED_STEPS.set(Some(0));

    // Act
    let selected = super::enumerate_join_order(&inputs);
    let steps = CHECKED_STEPS.take();

    // Assert
    assert_eq!(selected, [3, 2, 1, 0]);
    assert_eq!(steps, Some(4 + 12 + 24 + 24));
}
