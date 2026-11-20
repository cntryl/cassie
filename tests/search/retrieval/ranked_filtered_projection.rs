//! The indexed ranked filtered path must return the same projected rows as the exact path.

use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

use crate::support_ranked_search::{
    load_corpus_in_process, tenant_corpus, SearchCounters, QUERY, TOTAL_DOCUMENTS,
};

#[test]
fn should_return_identical_projected_rows_from_the_indexed_with_exact_paths() {
    // Arrange
    let mut config = crate::support_sql::with_test_root_password(
        CassieRuntimeConfig::from_env().expect("config"),
    );
    config.limits.execution_result_cache_enabled =
        cassie::config::ExecutionResultCacheEnabled::disabled();
    let cassie = Cassie::new_in_memory_with_config(config).expect("cassie");
    cassie.startup().expect("startup");
    load_corpus_in_process(&cassie, &tenant_corpus(20));
    let session = cassie.create_session("root", None);
    let sql = "SELECT _id, body, snippet(body, $1) AS hit, search_score(body, $1) AS score \
               FROM docs WHERE tenant = 'a' AND search(body, $1) AND status = 'open' \
               ORDER BY score DESC, _id ASC LIMIT 4 OFFSET 1";
    let params = || vec![Value::String(QUERY.to_string())];
    let before = SearchCounters::read(&cassie);

    // Act
    let indexed = cassie
        .execute_sql(&session, sql, params())
        .expect("indexed query");
    let indexed_delta = SearchCounters::read(&cassie).since(before);
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("begin");
    cassie
        .execute_sql(
            &session,
            &format!(
                "UPDATE docs SET status = 'closed' WHERE _id = 'd{:03}'",
                TOTAL_DOCUMENTS - 1
            ),
            vec![],
        )
        .expect("unrelated write forces the transaction overlay");
    let exact = cassie
        .execute_sql(&session, sql, params())
        .expect("exact query");
    let exact_delta = SearchCounters::read(&cassie).since(before);

    // Assert
    assert_eq!(indexed.rows().len(), 4);
    assert_eq!(indexed.rows(), exact.rows());
    assert_eq!(indexed_delta.row_scan_fallbacks, 0);
    assert!(exact_delta.row_scan_fallbacks > 0);
    assert!(
        matches!(&indexed.rows()[0][2], Value::String(hit) if hit.contains("<mark>")),
        "snippets are rendered from the fetched candidate"
    );
}
