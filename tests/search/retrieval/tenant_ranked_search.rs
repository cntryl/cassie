//! #1369: tenant-filtered lexical ranking end to end over pgwire prepared statements, checked
//! against an independent corpus-wide BM25 oracle (`support/ranked_search.rs`).
//! #1368: hand-checkable BM25 corpus with ties, structured filters and a frequency-changing update.

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::midge::adapter::StorageFamily;
use cntryl_midge::{TransactionMode, WriteOptions};

use crate::support_ranked_search::{
    assert_page_matches, bm25_scores, exec_in_process, fallback_reason_count,
    load_corpus_in_process, ranked_page, tenant_corpus, SearchCounters, TenantDoc, WireFixture,
    QUERY, SCORE_TOLERANCE, TOTAL_DOCUMENTS,
};

const TIE_ORDERED: &str = "SELECT _id, search_score(body, $1) AS score FROM docs \
    WHERE search(body, $1) AND tenant = $2 AND status = $3 \
    ORDER BY score DESC, _id ASC LIMIT $4 OFFSET $5";
const SCORE_ONLY_ORDERED: &str = "SELECT _id, search_score(body, $1) AS score FROM docs \
    WHERE status = $3 AND search(body, $1) AND tenant = $2 \
    ORDER BY score DESC LIMIT $4 OFFSET $5";
const TERMS: [&str; 2] = ["harbor", "cobalt"];

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn uncached_config() -> CassieRuntimeConfig {
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("config"),
    );
    config.limits.execution_result_cache_enabled =
        cassie::config::ExecutionResultCacheEnabled::disabled();
    config
}

async fn loaded_fixture(corpus: &[TenantDoc]) -> WireFixture {
    let cassie = Cassie::new_in_memory_with_config(uncached_config()).expect("cassie");
    cassie.startup().expect("startup");
    let fixture = WireFixture::start(cassie).await;
    fixture.load_corpus(corpus);
    fixture
}

async fn tenant_page(
    fixture: &WireFixture,
    sql: &str,
    limit: i64,
    offset: i64,
) -> (Vec<(String, f64)>, SearchCounters) {
    let before = SearchCounters::read(&fixture.cassie);
    let rows = fixture
        .scored(sql, &[QUERY, "a", "open"], &[limit, offset])
        .await;
    (rows, SearchCounters::read(&fixture.cassie).since(before))
}

fn eligible_ids(corpus: &[TenantDoc]) -> Vec<String> {
    corpus
        .iter()
        .filter(|doc| doc.eligible())
        .map(|doc| doc.id.clone())
        .collect()
}

async fn assert_tenant_pages(eligible: usize) {
    // Arrange
    let corpus = tenant_corpus(eligible);
    let scores = bm25_scores(&corpus, &TERMS);
    let permitted = eligible_ids(&corpus);
    let fixture = loaded_fixture(&corpus).await;

    for (limit, offset) in [(5_usize, 0_usize), (5, 5), (10, 3), (50, 0)] {
        for sql in [TIE_ORDERED, SCORE_ONLY_ORDERED] {
            // Act
            let (actual, delta) = tenant_page(
                &fixture,
                sql,
                i64::try_from(limit).unwrap(),
                i64::try_from(offset).unwrap(),
            )
            .await;

            // Assert
            let expected = ranked_page(&scores, &corpus, TenantDoc::eligible, limit, offset);
            assert_page_matches(&actual, &expected, SCORE_TOLERANCE);
            assert!(
                actual.iter().all(|(id, _)| permitted.contains(id)),
                "no cross-tenant or ineligible-status row may be returned"
            );
            assert_eq!(delta.row_scan_fallbacks, 0, "indexed path expected");
            assert!(delta.posting_reads > 0);
            assert_eq!(usize::try_from(delta.results).unwrap(), actual.len());
            assert_eq!(usize::try_from(delta.candidates).unwrap(), scores.len());
        }
    }
    fixture.stop().await;
}

#[test]
fn should_rank_tenant_status_pages_like_global_bm25_given_one_percent_eligible() {
    runtime().block_on(assert_tenant_pages(2));
}

#[test]
fn should_rank_tenant_status_pages_like_global_bm25_given_ten_percent_eligible() {
    runtime().block_on(assert_tenant_pages(20));
}

#[test]
fn should_rank_tenant_status_pages_like_global_bm25_given_half_eligible() {
    runtime().block_on(assert_tenant_pages(100));
}

#[test]
fn should_fetch_only_eligible_rows_given_a_selective_tenant_search() {
    runtime().block_on(async {
        // Arrange
        let corpus = tenant_corpus(2);
        let fixture = loaded_fixture(&corpus).await;

        // Act
        let (rows, delta) = tenant_page(&fixture, TIE_ORDERED, 2, 0).await;

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(delta.row_scan_fallbacks, 0);
        assert_eq!(
            delta.row_fetches, 2,
            "only the ranked eligible rows are fetched"
        );
        assert!(
            usize::try_from(delta.row_fetches).unwrap() < TOTAL_DOCUMENTS,
            "fewer source fetches than a row scan over the collection"
        );
        assert!(delta.posting_reads > 0);
        fixture.stop().await;
    });
}

#[test]
fn should_keep_eligibility_moving_scores_given_a_term_change_in_another_tenant() {
    runtime().block_on(async {
        // Arrange
        let mut corpus = tenant_corpus(20);
        let fixture = loaded_fixture(&corpus).await;
        let (before, _) = tenant_page(&fixture, TIE_ORDERED, 100, 0).await;

        // Act
        let padding = "pad pad pad pad pad pad pad pad pad pad pad pad";
        fixture
            .exec(&format!(
                "UPDATE docs SET body = '{padding}' WHERE tenant = 'b' AND status = 'closed'"
            ))
            .await;
        for doc in corpus
            .iter_mut()
            .filter(|doc| doc.tenant == "b" && doc.status == "closed")
        {
            doc.body = padding.to_string();
        }
        let (after, delta) = tenant_page(&fixture, TIE_ORDERED, 100, 0).await;

        // Assert
        let scores = bm25_scores(&corpus, &TERMS);
        let expected = ranked_page(&scores, &corpus, TenantDoc::eligible, 100, 0);
        assert_page_matches(&after, &expected, SCORE_TOLERANCE);
        let membership = |rows: &[(String, f64)]| {
            rows.iter()
                .map(|row| row.0.clone())
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(membership(&before), membership(&after));
        assert!(
            before.iter().any(|(id, score)| after
                .iter()
                .any(|(other, moved)| other == id && (moved - score).abs() > 1e-6)),
            "global document frequency changed, so scores must move"
        );
        assert_eq!(delta.row_scan_fallbacks, 0);
        fixture.stop().await;
    });
}

fn expected_page(corpus: &[TenantDoc]) -> Vec<(String, f64)> {
    ranked_page(
        &bm25_scores(corpus, &TERMS),
        corpus,
        TenantDoc::eligible,
        100,
        0,
    )
}

#[test]
fn should_follow_committed_moves_ignoring_rolled_back_moves_given_group_changes() {
    runtime().block_on(async {
        // Arrange
        let mut corpus = tenant_corpus(20);
        let fixture = loaded_fixture(&corpus).await;
        let baseline = tenant_page(&fixture, TIE_ORDERED, 100, 0).await.0;

        // Act: an open transaction sees its own move through the labelled exact fallback.
        fixture.exec("BEGIN").await;
        fixture
            .exec("UPDATE docs SET tenant = 'b' WHERE _id = 'd000'")
            .await;
        let mut moved = corpus.clone();
        moved[0].tenant = "b";
        let (in_transaction, in_transaction_delta) =
            tenant_page(&fixture, TIE_ORDERED, 100, 0).await;
        fixture.exec("ROLLBACK").await;
        let (rolled_back, rolled_back_delta) = tenant_page(&fixture, TIE_ORDERED, 100, 0).await;
        fixture
            .exec("UPDATE docs SET status = 'closed' WHERE _id = 'd001'")
            .await;
        let original = corpus.clone();
        corpus[1].status = "closed";
        let (committed, committed_delta) = tenant_page(&fixture, TIE_ORDERED, 100, 0).await;

        // Assert
        assert_page_matches(&baseline, &expected_page(&original), SCORE_TOLERANCE);
        assert_page_matches(&in_transaction, &expected_page(&moved), SCORE_TOLERANCE);
        assert!(in_transaction_delta.row_scan_fallbacks > 0);
        assert!(in_transaction.iter().all(|row| row.0 != "d000"));
        assert_page_matches(&rolled_back, &expected_page(&original), SCORE_TOLERANCE);
        assert_eq!(rolled_back_delta.row_scan_fallbacks, 0);
        assert_page_matches(&committed, &expected_page(&corpus), SCORE_TOLERANCE);
        assert!(committed.iter().all(|row| row.0 != "d001"));
        assert_eq!(committed_delta.row_scan_fallbacks, 0);
        fixture.stop().await;
    });
}

fn corrupt_fulltext_manifest(cassie: &Cassie) {
    let collection = cassie
        .catalog
        .get_schema("docs")
        .map_or_else(|| "docs".to_string(), |schema| schema.collection);
    let prefix = cassie
        .midge
        .fulltext_artifact_prefix_for_diagnostics(&collection, "docs_fts")
        .expect("full-text artifact prefix");
    let key = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, &prefix)
        .expect("scan artifacts")
        .into_iter()
        .find_map(|(key, value)| value.starts_with(b"FTM1").then_some(key))
        .expect("full-text manifest artifact");
    let mut tx = cassie
        .midge
        .data_tx(TransactionMode::ReadWrite)
        .expect("transaction");
    tx.put(key, b"corrupt".to_vec(), None).expect("damage");
    tx.commit(WriteOptions::sync()).expect("commit damage");
}

#[test]
fn should_stay_consistent_after_reopen_with_damage_given_a_committed_group_move() {
    runtime().block_on(async {
        // Arrange
        let directory = crate::support_temp_dirs::IsolatedDataDir::new("tenant-ranked-reopen");
        let mut corpus = tenant_corpus(20);
        {
            let cassie =
                Cassie::new_local_with_data_dir_and_config(directory.path(), uncached_config())
                    .expect("open local");
            cassie.startup().expect("startup");
            load_corpus_in_process(&cassie, &corpus);
            exec_in_process(
                &cassie,
                "UPDATE docs SET status = 'closed' WHERE _id = 'd001'",
            );
        }
        corpus[1].status = "closed";
        let expected = expected_page(&corpus);

        // Act
        let cassie =
            Cassie::new_local_with_data_dir_and_config(directory.path(), uncached_config())
                .expect("reopen local");
        cassie.startup().expect("startup after reopen");
        let fixture = WireFixture::start(cassie).await;
        let (reopened, reopened_delta) = tenant_page(&fixture, TIE_ORDERED, 100, 0).await;
        corrupt_fulltext_manifest(&fixture.cassie);
        let reasons_before = fallback_reason_count(&fixture.cassie, "authoritative_row_scan");
        let (damaged, damaged_delta) = tenant_page(&fixture, TIE_ORDERED, 100, 0).await;

        // Assert
        assert_page_matches(&reopened, &expected, SCORE_TOLERANCE);
        assert_eq!(reopened_delta.row_scan_fallbacks, 0);
        assert_page_matches(&damaged, &expected, SCORE_TOLERANCE);
        assert!(damaged_delta.row_scan_fallbacks > 0);
        assert!(fallback_reason_count(&fixture.cassie, "authoritative_row_scan") > reasons_before);
        fixture.stop().await;
    });
}

fn hand_corpus() -> Vec<TenantDoc> {
    [
        ("b", "active", "alpha beta"),
        ("a", "active", "alpha beta"),
        ("c", "active", "alpha alpha alpha"),
        ("d", "active", "beta gamma"),
        ("z", "inactive", "alpha alpha"),
        ("y", "inactive", "noise gamma"),
    ]
    .iter()
    .map(|(id, status, body)| TenantDoc {
        id: (*id).to_string(),
        tenant: "a",
        status,
        body: (*body).to_string(),
        embedding: [0.0, 0.0],
    })
    .collect()
}

#[test]
fn should_rank_a_hand_checkable_corpus_with_ties_filters_plus_a_frequency_update() {
    runtime().block_on(async {
        // Arrange: n=6, avgdl=13/6, df(alpha)=4, idf=ln(7/4.5). a and b tie (tf=1, dl=2) and c
        // (tf=3, dl=3) leads; z is excluded by status. The update raises df(alpha) to 5.
        let original = hand_corpus();
        let mut corpus = hand_corpus();
        let fixture = loaded_fixture(&corpus).await;
        let sql = "SELECT _id, search_score(body, $1) AS score FROM docs \
                   WHERE search(body, $1) AND status = $2 \
                   ORDER BY score DESC, _id ASC LIMIT $3 OFFSET $4";
        let run = |limit: i64, offset: i64| {
            let fixture = &fixture;
            async move {
                let before = SearchCounters::read(&fixture.cassie);
                let rows = fixture
                    .scored(sql, &["alpha", "active"], &[limit, offset])
                    .await;
                (rows, SearchCounters::read(&fixture.cassie).since(before))
            }
        };

        // Act
        let (first, first_delta) = run(10, 0).await;
        let (top_two, top_two_delta) = run(2, 0).await;
        let (second_page, _) = run(2, 1).await;
        fixture
            .exec("UPDATE docs SET body = 'alpha beta' WHERE _id = 'y'")
            .await;
        corpus[5].body = "alpha beta".to_string();
        let (updated, updated_delta) = run(10, 0).await;

        // Assert
        let ids = |rows: &[(String, f64)]| rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>();
        let active = |doc: &TenantDoc| doc.status == "active";
        assert_eq!(ids(&first), ["c", "a", "b"]);
        assert_eq!(ids(&top_two), ["c", "a"]);
        assert_eq!(ids(&second_page), ["a", "b"]);
        let idf = (7.0_f64 / 4.5).ln();
        assert!(
            (first[0].1 - idf * 3.0 * 2.2 / (3.0 + 1.2 * (0.25 + 0.75 * 3.0 / (13.0 / 6.0)))).abs()
                < 1e-9
        );
        assert!(first[1].1.to_bits() == first[2].1.to_bits(), "exact tie");
        assert_page_matches(
            &first,
            &ranked_page(
                &bm25_scores(&original, &["alpha"]),
                &original,
                active,
                10,
                0,
            ),
            SCORE_TOLERANCE,
        );
        assert_eq!(ids(&updated), ["c", "a", "b"]);
        assert_page_matches(
            &updated,
            &ranked_page(&bm25_scores(&corpus, &["alpha"]), &corpus, active, 10, 0),
            SCORE_TOLERANCE,
        );
        assert!(
            updated[0].1 < first[0].1,
            "a corpus-frequency change must lower every alpha score"
        );
        assert_eq!(top_two.len(), 2, "top-k is bounded by LIMIT");
        for delta in [first_delta, top_two_delta, updated_delta] {
            assert_eq!(delta.row_scan_fallbacks, 0, "indexed path expected");
            assert!(delta.posting_reads > 0);
        }
        fixture.stop().await;
    });
}
