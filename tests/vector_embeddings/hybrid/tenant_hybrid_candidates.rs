//! #1363: tenant-filtered hybrid retrieval composes the persisted postings with the scalar-index
//! subset instead of the exact row scan. `hybrid_score` exists only for rows with a lexical
//! match and takes its vector component from the fetched row, so postings restricted to the
//! subset are the exact candidate set and no vector candidate window is read (`ann_reads` stays
//! zero with or without an ANN index). Expected scores come from the documented contract in
//! `docs/performance-contracts.md`, computed independently by the shared oracle.

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::types::Value;

use crate::support_ranked_search::{
    assert_page_matches, create_vector_index_in_process, hybrid_scores, load_corpus_in_process,
    ranked_page, tenant_corpus, TenantDoc, HYBRID_TOLERANCE, QUERY, TOTAL_DOCUMENTS,
};
use crate::support_vector_filtered::{uncached_config, Counters};

const TERMS: [&str; 2] = ["harbor", "cobalt"];
const NEAREST: [f64; 2] = [1.0, 0.0];
const HYBRID_PAGE: &str =
    "SELECT _id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score \
    FROM docs WHERE search(body, $1) AND tenant = $3 AND status = $4 \
    ORDER BY score DESC LIMIT $5 OFFSET $6";

fn open(
    corpus: &[TenantDoc],
    with_ann: bool,
    config: cassie::config::CassieRuntimeConfig,
) -> (Cassie, CassieSession) {
    let cassie = Cassie::new_in_memory_with_config(config).expect("cassie");
    cassie.startup().expect("startup");
    load_corpus_in_process(&cassie, corpus);
    if with_ann {
        create_vector_index_in_process(&cassie);
    }
    let session = cassie.create_session("root", None);
    (cassie, session)
}

fn page_params(limit: usize, offset: usize) -> Vec<Value> {
    vec![
        Value::String(QUERY.to_string()),
        Value::String("[1,0]".to_string()),
        Value::String("a".to_string()),
        Value::String("open".to_string()),
        Value::Int64(i64::try_from(limit).expect("limit")),
        Value::Int64(i64::try_from(offset).expect("offset")),
    ]
}

fn run(
    cassie: &Cassie,
    session: &CassieSession,
    (limit, offset): (usize, usize),
) -> Result<Vec<(String, f64)>, CassieError> {
    cassie
        .execute_sql(session, HYBRID_PAGE, page_params(limit, offset))
        .map(|result| {
            result
                .rows()
                .iter()
                .map(|row| match (&row[0], &row[1]) {
                    (Value::String(id), Value::Float64(score)) => (id.clone(), *score),
                    _ => panic!("unexpected row shape"),
                })
                .collect()
        })
}

fn expected_page(corpus: &[TenantDoc], limit: usize, offset: usize) -> Vec<(String, f64)> {
    ranked_page(
        &hybrid_scores(corpus, &TERMS, NEAREST),
        corpus,
        TenantDoc::eligible,
        limit,
        offset,
    )
}

fn assert_selectivity(eligible: usize) {
    for with_ann in [false, true] {
        // Arrange
        let corpus = tenant_corpus(eligible);
        let (cassie, session) = open(&corpus, with_ann, uncached_config());

        for (limit, offset) in [(5_usize, 0_usize), (5, 5), (10, 3), (50, 0)] {
            let before = Counters::read(&cassie);

            // Act
            let actual = run(&cassie, &session, (limit, offset)).expect("hybrid page");
            let delta = Counters::read(&cassie).since(&before);

            // Assert
            assert_page_matches(
                &actual,
                &expected_page(&corpus, limit, offset),
                HYBRID_TOLERANCE,
            );
            let context = format!("ann={with_ann} eligible={eligible} page=({limit},{offset})");
            assert_eq!(delta.hybrid_bounded_exact, 1, "{context}: {delta:?}");
            assert_eq!(delta.hybrid_row_scan_fallbacks, 0, "{context}");
            assert_eq!(
                delta.eligible_subset,
                u64::try_from(eligible).unwrap(),
                "{context}"
            );
            assert!(
                delta.row_fetches <= u64::try_from(eligible).unwrap(),
                "{context}: fetches {} must stay within the scalar subset",
                delta.row_fetches
            );
            assert!(
                delta.row_fetches < u64::try_from(TOTAL_DOCUMENTS).unwrap(),
                "{context}: fewer fetches than the unrestricted exact scan"
            );
        }
    }
}

#[test]
fn should_serve_hybrid_tenant_pages_from_the_scalar_subset_given_one_percent_eligible() {
    assert_selectivity(2);
}

#[test]
fn should_serve_hybrid_tenant_pages_from_the_scalar_subset_given_ten_percent_eligible() {
    assert_selectivity(20);
}

#[test]
fn should_serve_hybrid_tenant_pages_from_the_scalar_subset_given_half_eligible() {
    assert_selectivity(100);
}

#[test]
fn should_follow_committed_changes_in_hybrid_tenant_pages() {
    // Arrange
    let mut corpus = tenant_corpus(20);
    let (cassie, session) = open(&corpus, true, uncached_config());
    let before = run(&cassie, &session, (50, 0)).expect("baseline");

    // Act
    cassie
        .execute_sql(
            &session,
            "UPDATE docs SET status = 'closed' WHERE _id = 'd000'",
            vec![],
        )
        .expect("update");
    cassie
        .execute_sql(&session, "DELETE FROM docs WHERE _id = 'd001'", vec![])
        .expect("delete");
    corpus[0].status = "closed";
    corpus.retain(|doc| doc.id != "d001");
    let counters = Counters::read(&cassie);
    let after = run(&cassie, &session, (50, 0)).expect("after change");
    let delta = Counters::read(&cassie).since(&counters);

    // Assert
    assert!(before.iter().any(|(id, _)| id == "d000"));
    assert!(before.iter().any(|(id, _)| id == "d001"));
    assert_page_matches(&after, &expected_page(&corpus, 50, 0), HYBRID_TOLERANCE);
    assert!(
        after.iter().all(|(id, _)| id != "d000" && id != "d001"),
        "a closed or deleted row must not appear"
    );
    assert_eq!(delta.hybrid_bounded_exact, 1);
}

#[test]
fn should_keep_hybrid_snapshots_isolated_across_a_rolled_back_transaction() {
    // Arrange
    let corpus = tenant_corpus(20);
    let (cassie, session) = open(&corpus, true, uncached_config());
    let other = cassie.create_session("root", None);
    let mut moved = corpus.clone();
    moved[0].tenant = "b";

    // Act
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    cassie
        .execute_sql(
            &session,
            "UPDATE docs SET tenant = 'b' WHERE _id = 'd000'",
            vec![],
        )
        .expect("move");
    let before = Counters::read(&cassie);
    let inside = run(&cassie, &session, (50, 0)).expect("inside");
    let inside_delta = Counters::read(&cassie).since(&before);
    let concurrent = run(&cassie, &other, (50, 0)).expect("concurrent");
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback");
    let before = Counters::read(&cassie);
    let after = run(&cassie, &session, (50, 0)).expect("after rollback");
    let after_delta = Counters::read(&cassie).since(&before);

    // Assert
    assert_page_matches(&inside, &expected_page(&moved, 50, 0), HYBRID_TOLERANCE);
    assert_eq!(
        inside_delta.hybrid_bounded_exact, 0,
        "a transaction overlay never uses the committed-index strategy"
    );
    assert_page_matches(
        &concurrent,
        &expected_page(&corpus, 50, 0),
        HYBRID_TOLERANCE,
    );
    assert_page_matches(&after, &expected_page(&corpus, 50, 0), HYBRID_TOLERANCE);
    assert_eq!(after_delta.hybrid_bounded_exact, 1);
}

#[test]
fn should_fail_hybrid_with_a_resource_limit_rather_than_leak_rows_given_a_lowered_budget() {
    // Arrange
    let corpus = tenant_corpus(100);
    let expected = expected_page(&corpus, 50, 0);
    let mut outcomes = Vec::new();

    // Act
    for budget in [8_usize * 1_024, 32 * 1_024, 128 * 1_024, 16 * 1_024 * 1_024] {
        let mut config = uncached_config();
        config.limits.query_memory_budget_bytes = budget;
        let (cassie, session) = open(&corpus, false, config);
        let result = run(&cassie, &session, (50, 0));
        let accounted = cassie.metrics()["query"]["current_accounted_memory_bytes"]
            .as_u64()
            .expect("accounted bytes");
        outcomes.push((budget, result, accounted));
    }

    // Assert
    for (budget, result, accounted) in &outcomes {
        assert_eq!(
            *accounted, 0,
            "budget {budget}: reservations must be released"
        );
        match result {
            Ok(page) => assert_page_matches(page, &expected, HYBRID_TOLERANCE),
            Err(error) => assert!(
                matches!(error, CassieError::ResourceLimit(_)),
                "budget {budget}: a lowered budget fails with a resource limit"
            ),
        }
    }
    assert!(
        outcomes[0].1.is_err(),
        "the smallest budget must be rejected"
    );
    assert!(
        outcomes.last().is_some_and(|(_, result, _)| result.is_ok()),
        "the largest budget succeeds"
    );
}
