//! #1369: tenant-filtered hybrid ranking over pgwire prepared statements. The expected final
//! score is computed independently from the documented contract in `docs/performance-contracts.md`
//! (corpus-wide BM25 combined as `0.65 * bm25 + 0.35 / (1 + L2)`, rounded to 12 places) and
//! eligible membership comes from the tenant and status predicates. The vector candidate stage
//! (ANN window fill, recall, work bounds) belongs to #1363 and is not exercised here: this fixture
//! has no ANN index, so vector candidates are exact and `ann_reads_total` stays zero.

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::midge::adapter::StorageFamily;
use cntryl_midge::{TransactionMode, WriteOptions};

use crate::support_ranked_search::{
    assert_page_matches, create_vector_index_in_process, exec_in_process, hybrid_scores,
    load_corpus_in_process, ranked_page, tenant_corpus, TenantDoc, WireFixture, HYBRID_TOLERANCE,
    QUERY,
};

const TERMS: [&str; 2] = ["harbor", "cobalt"];
const NEAREST: [f64; 2] = [1.0, 0.0];
const HYBRID_PAGE: &str =
    "SELECT _id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score \
    FROM docs WHERE search(body, $1) AND tenant = $3 AND status = $4 \
    ORDER BY score DESC LIMIT $5 OFFSET $6";

#[derive(Clone, Copy, Debug, Default)]
struct HybridCounters {
    posting_reads: u64,
    ann_reads: u64,
    row_fetches: u64,
    row_scan_fallbacks: u64,
    candidates: u64,
}

impl HybridCounters {
    fn read(cassie: &Cassie) -> Self {
        let metrics = cassie.metrics();
        let hybrid = &metrics["hybrid"];
        let field = |name: &str| hybrid[name].as_u64().unwrap_or(0);
        Self {
            posting_reads: field("posting_reads_total"),
            ann_reads: field("ann_reads_total"),
            row_fetches: field("candidate_row_fetches_total"),
            row_scan_fallbacks: metrics["search"]["row_scan_fallback_total"]
                .as_u64()
                .unwrap_or(0),
            candidates: field("candidate_count_total"),
        }
    }

    fn since(self, before: Self) -> Self {
        Self {
            posting_reads: self.posting_reads - before.posting_reads,
            ann_reads: self.ann_reads - before.ann_reads,
            row_fetches: self.row_fetches - before.row_fetches,
            row_scan_fallbacks: self.row_scan_fallbacks - before.row_scan_fallbacks,
            candidates: self.candidates.wrapping_sub(before.candidates),
        }
    }
}

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

async fn hybrid_page(
    fixture: &WireFixture,
    limit: i64,
    offset: i64,
) -> (Vec<(String, f64)>, HybridCounters) {
    let before = HybridCounters::read(&fixture.cassie);
    let rows = fixture
        .scored(
            HYBRID_PAGE,
            &[QUERY, "[1,0]", "a", "open"],
            &[limit, offset],
        )
        .await;
    (rows, HybridCounters::read(&fixture.cassie).since(before))
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

async fn assert_hybrid_pages(eligible: usize, with_ann: bool) {
    // Arrange
    let corpus = tenant_corpus(eligible);
    let permitted: Vec<String> = corpus
        .iter()
        .filter(|doc| doc.eligible())
        .map(|doc| doc.id.clone())
        .collect();
    let cassie = Cassie::new_in_memory_with_config(uncached_config()).expect("cassie");
    cassie.startup().expect("startup");
    let fixture = WireFixture::start(cassie).await;
    fixture.load_corpus(&corpus);
    if with_ann {
        create_vector_index_in_process(&fixture.cassie);
    }

    for (limit, offset) in [(5_usize, 0_usize), (5, 5), (10, 3), (50, 0)] {
        // Act
        let (actual, delta) = hybrid_page(
            &fixture,
            i64::try_from(limit).unwrap(),
            i64::try_from(offset).unwrap(),
        )
        .await;

        // Assert
        assert_page_matches(
            &actual,
            &expected_page(&corpus, limit, offset),
            HYBRID_TOLERANCE,
        );
        assert!(
            actual.iter().all(|(id, _)| permitted.contains(id)),
            "no cross-tenant or ineligible-status row may be returned"
        );
        if with_ann {
            assert!(
                delta.row_scan_fallbacks > 0 || (delta.ann_reads > 0 && delta.posting_reads > 0),
                "hybrid must report ANN and posting reads or a labelled exact fallback: {delta:?}"
            );
        } else {
            assert_eq!(delta.ann_reads, 0, "no ANN index is configured");
            assert!(
                delta.row_scan_fallbacks > 0,
                "tenant filter without ANN is the exact path"
            );
        }
        assert!(
            usize::try_from(delta.row_fetches).unwrap() <= corpus.len(),
            "row fetches stay within the collection"
        );
    }
    fixture.stop().await;
}

#[test]
fn should_rank_hybrid_tenant_pages_by_the_documented_contract_given_one_percent_eligible() {
    runtime().block_on(assert_hybrid_pages(2, false));
}

#[test]
fn should_rank_hybrid_tenant_pages_by_the_documented_contract_given_ten_percent_eligible() {
    runtime().block_on(assert_hybrid_pages(20, false));
}

#[test]
fn should_rank_hybrid_tenant_pages_by_the_documented_contract_given_half_eligible() {
    runtime().block_on(assert_hybrid_pages(100, false));
}

#[test]
fn should_keep_hybrid_eligibility_and_move_scores_given_a_term_change_in_another_tenant() {
    runtime().block_on(async {
        // Arrange
        let mut corpus = tenant_corpus(20);
        let cassie = Cassie::new_in_memory_with_config(uncached_config()).expect("cassie");
        cassie.startup().expect("startup");
        let fixture = WireFixture::start(cassie).await;
        fixture.load_corpus(&corpus);
        let (before, _) = hybrid_page(&fixture, 100, 0).await;

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
        let (after, _) = hybrid_page(&fixture, 100, 0).await;

        // Assert
        assert_page_matches(&after, &expected_page(&corpus, 100, 0), HYBRID_TOLERANCE);
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
            "global BM25 statistics changed, so hybrid scores must move"
        );
        fixture.stop().await;
    });
}

#[test]
fn should_follow_hybrid_moves_through_rollback_and_commit() {
    runtime().block_on(async {
        // Arrange
        let mut corpus = tenant_corpus(20);
        let cassie = Cassie::new_in_memory_with_config(uncached_config()).expect("cassie");
        cassie.startup().expect("startup");
        let fixture = WireFixture::start(cassie).await;
        fixture.load_corpus(&corpus);
        let baseline = hybrid_page(&fixture, 100, 0).await.0;
        let original = corpus.clone();

        // Act: an open transaction sees its own move through the labelled exact fallback.
        fixture.exec("BEGIN").await;
        fixture
            .exec("UPDATE docs SET tenant = 'b' WHERE _id = 'd000'")
            .await;
        let mut moved = corpus.clone();
        moved[0].tenant = "b";
        let (in_transaction, in_transaction_delta) = hybrid_page(&fixture, 100, 0).await;
        fixture.exec("ROLLBACK").await;
        let rolled_back = hybrid_page(&fixture, 100, 0).await.0;
        fixture
            .exec("UPDATE docs SET status = 'closed' WHERE _id = 'd001'")
            .await;
        corpus[1].status = "closed";
        let committed = hybrid_page(&fixture, 100, 0).await.0;

        // Assert
        assert_page_matches(
            &baseline,
            &expected_page(&original, 100, 0),
            HYBRID_TOLERANCE,
        );
        assert_page_matches(
            &in_transaction,
            &expected_page(&moved, 100, 0),
            HYBRID_TOLERANCE,
        );
        assert!(in_transaction_delta.row_scan_fallbacks > 0);
        assert_page_matches(
            &rolled_back,
            &expected_page(&original, 100, 0),
            HYBRID_TOLERANCE,
        );
        assert_page_matches(
            &committed,
            &expected_page(&corpus, 100, 0),
            HYBRID_TOLERANCE,
        );
        fixture.stop().await;
    });
}

#[test]
fn should_rank_hybrid_after_reopen_and_after_a_damaged_text_artifact() {
    runtime().block_on(async {
        // Arrange
        let directory = crate::support_temp_dirs::IsolatedDataDir::new("tenant-hybrid-reopen");
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

        // Act
        let cassie =
            Cassie::new_local_with_data_dir_and_config(directory.path(), uncached_config())
                .expect("reopen local");
        cassie.startup().expect("startup after reopen");
        let fixture = WireFixture::start(cassie).await;
        let reopened = hybrid_page(&fixture, 100, 0).await.0;
        damage_fulltext_manifest(&fixture.cassie);
        let (damaged, damaged_delta) = hybrid_page(&fixture, 100, 0).await;

        // Assert
        assert_page_matches(&reopened, &expected_page(&corpus, 100, 0), HYBRID_TOLERANCE);
        assert_page_matches(&damaged, &expected_page(&corpus, 100, 0), HYBRID_TOLERANCE);
        assert!(
            damaged_delta.row_scan_fallbacks > 0,
            "a damaged manifest must select the labelled exact fallback"
        );
        fixture.stop().await;
    });
}

fn damage_fulltext_manifest(cassie: &Cassie) {
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
fn should_rank_hybrid_tenant_pages_with_an_ann_index_given_one_percent_eligible() {
    runtime().block_on(assert_hybrid_pages(2, true));
}

#[test]
fn should_rank_hybrid_tenant_pages_with_an_ann_index_given_ten_percent_eligible() {
    runtime().block_on(assert_hybrid_pages(20, true));
}

#[test]
fn should_rank_hybrid_tenant_pages_with_an_ann_index_given_half_eligible() {
    runtime().block_on(assert_hybrid_pages(100, true));
}
