use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use std::fmt::Write as _;

#[path = "support/sql.rs"]
mod support;
use support::*;

fn seed_relation(cassie: &Cassie, name: &str, rows: usize) {
    let session = cassie.create_session("seed", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {name} (item_key INT, parent_key INT)"),
            vec![],
        )
        .expect("create memo relation");
    for id in 1..=rows {
        cassie
            .execute_sql(
                &session,
                &format!("INSERT INTO {name} (item_key, parent_key) VALUES ($1, $2)"),
                vec![
                    cassie::types::Value::Int64(id.try_into().expect("fixture id")),
                    cassie::types::Value::Int64(id.try_into().expect("fixture parent id")),
                ],
            )
            .expect("seed memo relation");
    }
    let canonical = canonical_relation_name("postgres", "public", name);
    let stats = cassie
        .midge
        .rebuild_cardinality_stats_for_collection(&canonical)
        .expect("cardinality stats");
    cassie.catalog.hydrate_cardinality_stats(&canonical, stats);
}

#[test]
fn should_enumerate_three_relation_inner_join_by_cardinality() {
    // Arrange
    with_fallback();
    let path = data_dir("memo_three_relation_order");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    seed_relation(&cassie, "memo_large", 8);
    seed_relation(&cassie, "memo_small", 1);
    seed_relation(&cassie, "memo_medium", 3);
    let session = cassie.create_session("tester", None);

    // Act
    let explained = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT memo_large.item_key FROM memo_large JOIN memo_small ON memo_large.parent_key = memo_small.item_key JOIN memo_medium ON memo_small.parent_key = memo_medium.item_key",
            vec![],
        )
        .expect("explain memo join");
    let plan = explained.rows[0][0].as_str().unwrap_or_default();

    // Assert
    assert!(plan.contains("join_enumeration=exhaustive"), "plan={plan}");
    assert!(
        plan.contains("join_order=memo_small>memo_medium>memo_large"),
        "plan={plan}"
    );

    let result = cassie
        .execute_sql(
            &session,
            "SELECT memo_large.item_key FROM memo_large JOIN memo_small ON memo_large.parent_key = memo_small.item_key JOIN memo_medium ON memo_small.parent_key = memo_medium.item_key",
            vec![],
        )
        .expect("execute reordered memo join");
    assert_eq!(result.rows, vec![vec![cassie::types::Value::Int64(1)]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_use_deterministic_fallback_order_when_statistics_are_missing() {
    // Arrange
    with_fallback();
    let path = data_dir("memo_missing_stats_order");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    for name in ["zebra", "alpha", "middle"] {
        cassie
            .execute_sql(&session, &format!("CREATE TABLE {name} (id INT)"), vec![])
            .expect("create relation without statistics");
    }

    // Act
    let explained = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT zebra.id FROM zebra JOIN alpha ON zebra.id = alpha.id JOIN middle ON alpha.id = middle.id",
            vec![],
        )
        .expect("explain missing statistics join");
    let plan = explained.rows[0][0].as_str().unwrap_or_default();

    // Assert
    assert!(plan.contains("join_enumeration=exhaustive"), "plan={plan}");
    assert!(
        plan.contains("join_order=alpha>middle>zebra"),
        "plan={plan}"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_preserve_outer_join_as_legality_barrier() {
    // Arrange
    with_fallback();
    let path = data_dir("memo_outer_join_barrier");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    for name in ["outer_left", "outer_right", "outer_tail"] {
        cassie
            .execute_sql(&session, &format!("CREATE TABLE {name} (id INT)"), vec![])
            .expect("create outer relation");
    }

    // Act
    let explained = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT outer_left.id FROM outer_left LEFT JOIN outer_right ON outer_left.id = outer_right.id JOIN outer_tail ON outer_left.id = outer_tail.id",
            vec![],
        )
        .expect("explain outer join barrier");
    let plan = explained.rows[0][0].as_str().unwrap_or_default();

    // Assert
    assert!(plan.contains("join_enumeration=none"), "plan={plan}");
    assert!(
        plan.contains("join_order=outer_left>outer_right>outer_tail"),
        "plan={plan}"
    );
    assert!(
        plan.contains("join_legality_barriers=left_outer"),
        "plan={plan}"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_enumerate_non_equality_inner_joins() {
    // Arrange
    with_fallback();
    let path = data_dir("memo_non_equality_order");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    seed_relation(&cassie, "range_large", 8);
    seed_relation(&cassie, "range_small", 1);
    seed_relation(&cassie, "range_medium", 3);
    let session = cassie.create_session("tester", None);

    // Act
    let explained = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT range_large.item_key FROM range_large JOIN range_small ON range_large.parent_key >= range_small.item_key JOIN range_medium ON range_small.parent_key <= range_medium.item_key",
            vec![],
        )
        .expect("explain non-equality memo join");
    let plan = explained.rows[0][0].as_str().unwrap_or_default();

    // Assert
    assert!(plan.contains("join_enumeration=exhaustive"), "plan={plan}");
    assert!(
        plan.contains("join_order=range_small>range_medium>range_large"),
        "plan={plan}"
    );
    assert!(
        plan.contains("join_fallback_reason=non_equi_predicate"),
        "plan={plan}"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_use_greedy_expansion_above_eight_relations() {
    // Arrange
    with_fallback();
    let path = data_dir("memo_greedy_nine_relations");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    for index in 1..=9 {
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE greedy_{index:02} (item_key INT)"),
                vec![],
            )
            .expect("create greedy relation");
    }
    let mut sql = "EXPLAIN SELECT greedy_09.item_key FROM greedy_09".to_string();
    for index in (1..9).rev() {
        let previous = index + 1;
        write!(
            &mut sql,
            " JOIN greedy_{index:02} ON greedy_{previous:02}.item_key = greedy_{index:02}.item_key"
        )
        .expect("append greedy join");
    }

    // Act
    let explained = cassie
        .execute_sql(&session, &sql, vec![])
        .expect("explain greedy memo join");
    let plan = explained.rows[0][0].as_str().unwrap_or_default();

    // Assert
    assert!(plan.contains("join_enumeration=greedy"), "plan={plan}");
    assert!(
        plan.contains(
            "join_order=greedy_01>greedy_02>greedy_03>greedy_04>greedy_05>greedy_06>greedy_07>greedy_08>greedy_09"
        ),
        "plan={plan}"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_explain_join_physical_properties_with_memory_bound() {
    // Arrange
    with_fallback();
    let path = data_dir("memo_physical_properties");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    let session = cassie.create_session("tester", None);
    for name in ["property_left", "property_right"] {
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {name} (item_key INT)"),
                vec![],
            )
            .expect("create property relation");
    }

    // Act
    let explained = cassie
        .execute_sql(
            &session,
            "EXPLAIN SELECT property_left.item_key FROM property_left JOIN property_right ON property_left.item_key = property_right.item_key ORDER BY property_left.item_key DESC LIMIT 2",
            vec![],
        )
        .expect("explain join properties");
    let plan = explained.rows[0][0].as_str().unwrap_or_default();

    // Assert
    assert!(
        plan.contains("join_required_columns=property_left.item_key,property_right.item_key"),
        "plan={plan}"
    );
    assert!(
        plan.contains("join_required_ordering=property_left.item_key:desc"),
        "plan={plan}"
    );
    assert!(plan.contains("join_parameterized=false"), "plan={plan}");
    assert!(plan.contains("join_rewindable=true"), "plan={plan}");
    assert!(plan.contains("join_bounded=true"), "plan={plan}");
    assert!(
        plan.contains("join_memory_bound=accounted_query_budget"),
        "plan={plan}"
    );
    let _ = std::fs::remove_dir_all(path);
}
