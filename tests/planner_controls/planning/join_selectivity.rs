//! Finite join-order choices and their unchanged SQL results, without timing claims.

use cassie as fixture_cassie;
use cassie::app::Cassie;
use cassie::catalog::{canonical_relation_name, CollectionCardinalityStats};
use cassie::planner::optimizer;
use cassie::sql::ast::{BinaryOp, Expr, QuerySource};
use cassie::types::Value;
use std::fmt::Write as _;

#[path = "../../support/join_cost_stats.rs"]
mod fixture;

const STAR: &str = "SELECT a.id FROM a JOIN c ON a.id = c.ak JOIN b ON b.id = c.bk";

fn source_order(source: &QuerySource) -> Vec<String> {
    match source {
        QuerySource::Join { left, right, .. } => {
            let mut names = source_order(left);
            names.extend(source_order(right));
            names
        }
        QuerySource::Collection(name) => vec![name.to_string()],
        _ => vec!["derived".to_string()],
    }
}

fn planned_order(sql: &str, stats: &fixture::Stats) -> Vec<String> {
    source_order(&optimizer::optimize_with_stats(fixture::plan(sql), stats).source)
}

fn predicate_atoms(source: &QuerySource) -> Vec<String> {
    fn append(expr: &Expr, atoms: &mut Vec<String>) {
        match expr {
            Expr::Binary {
                left,
                op: BinaryOp::And,
                right,
            } => {
                append(left, atoms);
                append(right, atoms);
            }
            Expr::BoolLiteral(true) => {}
            _ => atoms.push(serde_json::to_string(expr).expect("predicate identity")),
        }
    }
    let mut atoms = Vec::new();
    if let QuerySource::Join {
        left, right, on, ..
    } = source
    {
        atoms.extend(predicate_atoms(left));
        atoms.extend(predicate_atoms(right));
        append(on, &mut atoms);
    }
    atoms.sort();
    atoms
}

#[test]
fn should_choose_a_selective_join_before_a_smaller_cartesian_input() {
    // Arrange
    let stats = fixture::star_stats(true);

    // Act
    let order = planned_order(STAR, &stats);

    // Assert
    assert_eq!(order, ["a", "c", "b"]);
}

#[test]
fn should_change_join_order_when_only_usable_distinct_counts_change() {
    // Arrange
    let selective = fixture::star_stats(true);
    let duplicate_heavy = fixture::star_stats(false);

    // Act
    let selective_order = planned_order(STAR, &selective);
    let duplicate_order = planned_order(STAR, &duplicate_heavy);

    // Assert
    assert_eq!(selective_order, ["a", "c", "b"]);
    assert_eq!(duplicate_order, ["a", "b", "c"]);
}

#[test]
fn should_keep_a_cheaper_cartesian_prefix_for_duplicate_heavy_joins() {
    // Arrange
    let stats = fixture::star_stats(false);

    // Act
    let order = planned_order(STAR, &stats);

    // Assert
    assert_eq!(order, ["a", "b", "c"]);
}

#[test]
fn should_preserve_the_complete_join_predicate_bag() {
    // Arrange
    let stats = fixture::star_stats(true);
    let cases = [
        "SELECT a.id FROM a JOIN c ON a.id=c.ak AND a.id=c.ak JOIN b ON b.id=c.bk",
        "SELECT a.id FROM a JOIN c ON c.ak=a.id JOIN b ON c.bk=b.id",
        "SELECT a.id FROM a JOIN c ON a.id=c.ak JOIN b ON b.id=c.bk AND a.id=b.id",
    ];

    // Act
    for sql in cases {
        let before = fixture::plan(sql);
        let after = optimizer::optimize_with_stats(before.clone(), &stats);

        // Assert
        assert_eq!(source_order(&after.source), ["a", "c", "b"], "{sql}");
        assert_eq!(
            predicate_atoms(&after.source),
            predicate_atoms(&before.source)
        );
    }
}

#[test]
fn should_wait_for_a_whole_compound_on_expression_before_using_its_selectivity() {
    // Arrange
    let mut stats = fixture::star_stats(true);
    stats
        .get_mut("b")
        .expect("b")
        .fields
        .get_mut("id")
        .expect("id")
        .distinct_count = 1;
    let sql = "SELECT a.id FROM a JOIN b ON a.id=b.id JOIN c ON b.id=c.bk AND a.id=c.ak";

    // Act
    let before = fixture::plan(sql);
    let after = optimizer::optimize_with_stats(before.clone(), &stats);

    // Assert
    assert_eq!(source_order(&after.source), ["a", "b", "c"]);
    assert_eq!(
        predicate_atoms(&after.source),
        predicate_atoms(&before.source)
    );
}

fn matching_subexpressions(expr: &Expr, expected: &serde_json::Value) -> usize {
    let mut matches =
        usize::from(serde_json::to_value(expr).expect("whole expression identity") == *expected);
    expr.for_each_child(|child| matches += matching_subexpressions(child, expected));
    matches
}

#[test]
fn should_defer_a_whole_correlated_on_until_the_final_join() {
    // Arrange
    let before = fixture::plan(fixture::CORRELATED_ON_SQL);
    let QuerySource::Join {
        on: original_on, ..
    } = &before.source
    else {
        panic!("original final join");
    };
    let expected_on = serde_json::to_value(original_on).expect("original whole correlated ON");
    let stats = fixture::correlated_on_stats();

    // Act
    let after = optimizer::optimize_with_stats(before.clone(), &stats);

    // Assert
    assert_eq!(
        source_order(&after.source),
        ["probe_a_c", "probe_b_a", "probe_c_b"]
    );
    let QuerySource::Join {
        left, on: final_on, ..
    } = &after.source
    else {
        panic!("reordered final join");
    };
    let QuerySource::Join { on: first_on, .. } = left.as_ref() else {
        panic!("reordered prefix join");
    };
    assert_eq!(matching_subexpressions(first_on, &expected_on), 0);
    assert_eq!(matching_subexpressions(final_on, &expected_on), 1);
    assert_eq!(
        serde_json::to_value(first_on).expect("prefix ON"),
        serde_json::to_value(Expr::BoolLiteral(true)).expect("unfiltered prefix")
    );
    assert_eq!(
        predicate_atoms(&after.source),
        predicate_atoms(&before.source)
    );
}

#[test]
fn should_use_exact_qualified_base_statistics_before_suffix_matches() {
    // Arrange
    let sql = "SELECT s1.doc.id FROM s1.doc JOIN a ON s1.doc.id=a.id JOIN z ON a.id=z.id";
    let entries = [
        ("s1.doc", fixture::collection(8, &[])),
        ("doc", fixture::collection(1, &[])),
        ("s2.doc", fixture::collection(100, &[])),
        ("a", fixture::collection(2, &[])),
        ("z", fixture::collection(3, &[])),
    ];

    // Act
    for reversed in [false, true] {
        let mut values = entries.clone().to_vec();
        if reversed {
            values.reverse();
        }
        let stats = values
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        let order = planned_order(sql, &stats);

        // Assert
        assert_eq!(order, ["a", "z", "s1.doc"]);
    }
}

#[test]
fn should_use_a_stable_default_for_ambiguous_base_statistics() {
    // Arrange
    let sql = "SELECT doc.id FROM doc JOIN a ON doc.id=a.id JOIN z ON a.id=z.id";
    let entries = [
        ("s1.doc", fixture::collection(100, &[])),
        ("s2.doc", fixture::collection(200, &[])),
        ("a", fixture::collection(2, &[])),
        ("z", fixture::collection(3, &[])),
    ];

    // Act
    for reversed in [false, true] {
        let mut values = entries.clone().to_vec();
        if reversed {
            values.reverse();
        }
        let stats = values
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect();
        let order = planned_order(sql, &stats);

        // Assert
        assert_eq!(order, ["doc", "a", "z"]);
    }
}

#[test]
fn should_keep_a_quoted_relation_distinct_in_base_statistics() {
    // Arrange
    let sql = "SELECT \"Case\".id FROM \"Case\" JOIN a ON \"Case\".id=a.id JOIN z ON a.id=z.id";
    let stats = fixture::Stats::from([
        ("\"Case\"".to_string(), fixture::collection(1, &[])),
        ("case".to_string(), fixture::collection(100, &[])),
        ("a".to_string(), fixture::collection(2, &[])),
        ("z".to_string(), fixture::collection(3, &[])),
    ]);

    // Act
    let order = planned_order(sql, &stats);

    // Assert
    assert_eq!(order, ["\"Case\"", "a", "z"]);
}

#[test]
fn should_use_exact_quoted_field_statistics_for_selectivity() {
    // Arrange
    let cases = [
        (
            "SELECT a.\"id.with.dot\" FROM a JOIN c ON a.\"id.with.dot\"=c.\"source.key\" JOIN b ON b.id=c.bk",
            "id.with.dot", "dot", "source.key", "key",
        ),
        (
            "SELECT a.\"K\" FROM a JOIN c ON a.\"K\"=c.\"AK\" JOIN b ON b.id=c.bk",
            "K", "k", "AK", "ak",
        ),
    ];

    // Act
    let observations = cases.map(|(sql, a_key, a_decoy, c_key, c_decoy)| {
        let stats = fixture::Stats::from([
            (
                "a".to_string(),
                fixture::collection(10, &[(a_key, 10), (a_decoy, 1)]),
            ),
            ("b".to_string(), fixture::collection(100, &[("id", 100)])),
            (
                "c".to_string(),
                fixture::collection(1_000, &[(c_key, 1_000), (c_decoy, 1), ("bk", 1_000)]),
            ),
        ]);
        let before = fixture::plan(sql);
        let after = optimizer::optimize_with_stats(before.clone(), &stats);
        let QuerySource::Join {
            left: before_left, ..
        } = &before.source
        else {
            panic!("three-relation fixture");
        };
        let QuerySource::Join { on: before_on, .. } = before_left.as_ref() else {
            panic!("original a-c join");
        };
        let QuerySource::Join {
            left: after_left, ..
        } = &after.source
        else {
            panic!("three-relation result");
        };
        let QuerySource::Join { on: after_on, .. } = after_left.as_ref() else {
            panic!("first result join");
        };
        (
            sql,
            source_order(&after.source),
            serde_json::to_value(before_on).expect("original whole ON"),
            serde_json::to_value(after_on).expect("first whole ON"),
        )
    });

    // Assert
    for (sql, order, expected_on, first_on) in observations {
        assert_eq!(order, ["a", "c", "b"], "{sql}");
        assert_eq!(
            first_on, expected_on,
            "filtering must occur at the estimated first join: {sql}"
        );
    }
}

#[test]
fn should_use_unique_qualified_statistics_for_unqualified_sources() {
    // Arrange
    let stats = fixture::star_stats(true)
        .into_iter()
        .map(|(name, value)| (format!("postgres.public.{name}"), value))
        .collect();

    // Act
    let order = planned_order(STAR, &stats);

    // Assert
    assert_eq!(order, ["a", "c", "b"]);
}

#[test]
fn should_accept_either_measurement_signal_for_distinct_statistics() {
    // Arrange
    let signals = [(0, 100), (1, 0)];

    // Act
    for (samples, confidence) in signals {
        let mut stats = fixture::star_stats(true);
        for collection in stats.values_mut() {
            for field in collection.fields.values_mut() {
                field.sample_count = samples;
                field.confidence = confidence;
            }
        }
        let order = planned_order(STAR, &stats);

        // Assert
        assert_eq!(order, ["a", "c", "b"]);
    }
}

#[test]
fn should_decline_unusable_field_statistics_without_discarding_base_counts() {
    // Arrange
    type Mutation = fn(&mut CollectionCardinalityStats);
    let cases: [(&str, Mutation); 9] = [
        ("missing", |stats| stats.fields.clear()),
        ("unhydrated", |stats| stats.hydrated = false),
        ("stale", |stats| {
            for field in stats.fields.values_mut() {
                field.stale_reason = Some("test".to_string());
            }
        }),
        ("unmeasured", |stats| {
            for field in stats.fields.values_mut() {
                field.sample_count = 0;
                field.confidence = 0;
            }
        }),
        ("nullable", |stats| {
            for field in stats.fields.values_mut() {
                field.null_count = 1;
            }
        }),
        ("missing cells", |stats| {
            for field in stats.fields.values_mut() {
                field.missing_count = 1;
            }
        }),
        ("inconsistent", |stats| {
            for field in stats.fields.values_mut() {
                field.non_null_count = stats.row_count - 1;
            }
        }),
        ("zero distinct", |stats| {
            for field in stats.fields.values_mut() {
                field.distinct_count = 0;
            }
        }),
        ("excess distinct", |stats| {
            for field in stats.fields.values_mut() {
                field.distinct_count = stats.row_count + 1;
            }
        }),
    ];

    // Act
    for (label, mutate) in cases {
        let mut stats = fixture::star_stats(true);
        for collection in stats.values_mut() {
            mutate(collection);
        }
        let order = planned_order(STAR, &stats);

        // Assert
        assert_eq!(order, ["a", "b", "c"], "{label}");
    }
}

#[test]
fn should_keep_where_selectivity_out_of_join_intermediate_estimates() {
    // Arrange
    let stats = fixture::star_stats(true);

    // Act
    let orders = ["a.id=1", "b.id=1"]
        .map(|predicate| planned_order(&format!("{STAR} WHERE {predicate}"), &stats));

    // Assert
    assert_eq!(orders, [vec!["a", "c", "b"], vec!["a", "c", "b"]]);
}

#[test]
fn should_preserve_existing_join_reordering_legality_boundaries() {
    // Arrange
    let stats = fixture::star_stats(true);
    let cases = [
        "SELECT a.id FROM a LEFT JOIN c ON a.id=c.ak JOIN b ON b.id=c.bk",
        "SELECT a.id FROM a CROSS JOIN c JOIN b ON b.id=c.bk",
        "SELECT aa.id FROM a AS aa JOIN c ON aa.id=c.ak JOIN b ON b.id=c.bk",
        "SELECT a.id FROM a JOIN (SELECT ak,bk FROM c) AS d ON a.id=d.ak JOIN b ON b.id=d.bk",
        "SELECT a.id FROM a JOIN LATERAL (SELECT a.id AS ak) AS d ON a.id=d.ak JOIN b ON a.id=b.id",
    ];

    // Act
    for sql in cases {
        let before = fixture::plan(sql);
        let after = optimizer::optimize_with_stats(before.clone(), &stats);

        // Assert
        assert_eq!(
            serde_json::to_value(&after.source).expect("complete result source"),
            serde_json::to_value(&before.source).expect("complete original source"),
            "{sql}"
        );
    }
}

#[test]
fn should_use_the_same_selectivity_estimate_for_greedy_expansion() {
    // Arrange
    let mut stats = fixture::star_stats(true);
    let mut sql = STAR.to_string();
    for (index, name) in ["d", "e", "f", "g", "h", "i"].into_iter().enumerate() {
        let rows = u64::try_from(index + 2).expect("small fixture") * 1_000;
        stats.insert(name.to_string(), fixture::collection(rows, &[("id", rows)]));
        write!(sql, " JOIN {name} ON {name}.id=c.ak").expect("join SQL");
    }

    // Act
    let plan = optimizer::optimize_with_stats(fixture::plan(&sql), &stats);

    // Assert
    assert_eq!(optimizer::join_enumeration(&plan.source), "greedy");
    assert_eq!(
        source_order(&plan.source),
        ["a", "c", "b", "d", "e", "f", "g", "h", "i"]
    );
}

fn sql_fixture() -> Cassie {
    let cassie = super::support_sql::new_in_memory_cassie().expect("memory fixture");
    cassie.startup().expect("start fixture");
    let session = cassie.create_session("root", None);
    for sql in [
        "CREATE TABLE cost_a(id INT NOT NULL)",
        "CREATE TABLE cost_b(id INT NOT NULL)",
        "CREATE TABLE cost_c(ak INT NOT NULL,bk INT NOT NULL)",
        "INSERT INTO cost_a VALUES(1),(2)",
        "INSERT INTO cost_b VALUES(1),(2),(3)",
        "INSERT INTO cost_c VALUES(1,1),(2,2),(3,3),(4,4)",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("tiny fixture statement");
    }
    for name in ["cost_a", "cost_b", "cost_c"] {
        let canonical = canonical_relation_name("postgres", "public", name);
        let stats = cassie
            .midge
            .rebuild_cardinality_stats_for_collection(&canonical)
            .expect("fresh statistics");
        cassie.catalog.hydrate_cardinality_stats(&canonical, stats);
    }
    cassie
}

fn correlated_sql_fixture() -> Cassie {
    let cassie = super::support_sql::new_in_memory_cassie().expect("memory correlated fixture");
    cassie.startup().expect("start correlated fixture");
    let session = cassie.create_session("root", None);
    for sql in [
        "CREATE TABLE probe_b_a(id INT NOT NULL)",
        "CREATE TABLE probe_c_b(id INT NOT NULL,a_id INT NOT NULL)",
        "CREATE TABLE probe_a_c(id INT NOT NULL)",
        "CREATE TABLE probe_lookup(c_id INT NOT NULL,b_id INT NOT NULL)",
        "INSERT INTO probe_b_a VALUES(1),(2)",
        "INSERT INTO probe_c_b VALUES(10,1),(20,1),(30,2)",
        "INSERT INTO probe_a_c VALUES(1)",
        "INSERT INTO probe_lookup VALUES(1,10)",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("correlated fixture statement");
    }
    for name in ["probe_b_a", "probe_c_b", "probe_a_c"] {
        let canonical = canonical_relation_name("postgres", "public", name);
        let stats = cassie
            .midge
            .rebuild_cardinality_stats_for_collection(&canonical)
            .expect("fresh correlated statistics");
        cassie.catalog.hydrate_cardinality_stats(&canonical, stats);
    }
    cassie
}

#[test]
fn should_preserve_rows_after_reordering_a_correlated_on() {
    // Arrange
    let cassie = correlated_sql_fixture();
    let session = cassie.create_session("root", None);
    let expected = vec![vec![Value::Int64(1), Value::Int64(10), Value::Int64(1)]];
    let aliased = cassie.execute_sql(&session,
        "SELECT aa.id,bb.id,cc.id FROM probe_b_a AS aa JOIN probe_c_b AS bb ON aa.id=bb.a_id JOIN probe_a_c AS cc ON aa.id=cc.id AND EXISTS(SELECT 1 FROM probe_lookup WHERE probe_lookup.c_id=cc.id AND probe_lookup.b_id=bb.id) ORDER BY bb.id",
        vec![]).expect("accepted alias barrier control");
    assert_eq!(aliased.rows(), expected);
    let final_on = cassie.execute_sql(&session,
        "SELECT probe_b_a.id,probe_c_b.id,probe_a_c.id FROM probe_a_c JOIN probe_b_a ON probe_b_a.id=probe_a_c.id JOIN probe_c_b ON probe_b_a.id=probe_c_b.a_id AND EXISTS(SELECT 1 FROM probe_lookup WHERE probe_lookup.c_id=probe_a_c.id AND probe_lookup.b_id=probe_c_b.id) ORDER BY probe_c_b.id",
        vec![]).expect("accepted same-name final-placement control");
    assert_eq!(final_on.rows(), expected);
    let explained = cassie
        .execute_sql(
            &session,
            &format!("EXPLAIN {}", fixture::CORRELATED_ON_SQL),
            vec![],
        )
        .expect("accepted correlated query");
    let plan = explained.rows()[0][0].as_str().expect("text explain");
    assert!(
        plan.contains("join_order=probe_a_c>probe_b_a>probe_c_b"),
        "{plan}"
    );

    // Act
    let result = cassie.execute_sql(&session, fixture::CORRELATED_ON_SQL, vec![]);

    // Assert
    assert_eq!(
        result.expect("reordered correlated ON executes").rows(),
        expected
    );
}

#[test]
fn should_preserve_parameterized_rows_in_the_explained_selective_order() {
    // Arrange
    let cassie = sql_fixture();
    let session = cassie.create_session("root", None);
    let sql = "SELECT cost_a.id,cost_b.id FROM cost_a JOIN cost_c ON cost_a.id=cost_c.ak JOIN cost_b ON cost_b.id=cost_c.bk WHERE cost_b.id >= $1 ORDER BY cost_a.id";

    // Act
    let explained = cassie
        .execute_sql(&session, &format!("EXPLAIN {sql}"), vec![Value::Int64(1)])
        .expect("explain selective join");
    let first = cassie
        .execute_sql(&session, sql, vec![Value::Int64(1)])
        .expect("first parameter");
    let second = cassie
        .execute_sql(&session, sql, vec![Value::Int64(2)])
        .expect("second parameter");

    // Assert
    let text = explained.rows()[0][0].as_str().expect("text explain");
    assert!(text.contains("join_order=cost_a>cost_c>cost_b"), "{text}");
    assert_eq!(
        first.rows(),
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)]
        ]
    );
    assert_eq!(second.rows(), vec![vec![Value::Int64(2), Value::Int64(2)]]);
}
