//! Agreement of indexed and authoritative `'query'` execution, expansion
//! limits and reopening a disk-backed store.

use super::fulltext_query_support::{
    authoritative, corpus_cassie, corpus_table, expect, indexed, insert, metric, run,
};
use super::support_sql as support;

use cassie::app::Cassie;
use cassie::search::query::limits::{MAX_EXPANDED_TERMS, MAX_EXPANSIONS_PER_ITEM};
use cassie::types::Value;

const AGREEMENT_QUERIES: [&str; 14] = [
    "\"red shoes\"",
    "\"red shoes\"~2",
    "red shoes",
    "red -running",
    "shoe*",
    "colr~",
    "(red OR blue) running",
    "\"alpha the beta\"",
    "alpha-beta",
    "caf*",
    "\"au lait\"",
    "na\u{ef}ve~",
    "shoes -sale",
    "\"ideas\"",
];

#[test]
fn should_agree_across_punctuation_null_with_unicode_given_indexed_and_authoritative_execution() {
    // Arrange
    let cassie = corpus_table();
    insert(&cassie, "n01", None);
    insert(&cassie, "n02", Some("red,shoes; (red) shoes!! -- running"));
    insert(&cassie, "n03", Some("RED SHOES"));
    insert(&cassie, "n04", Some("Caf\u{c9} AU LAIT"));
    insert(&cassie, "n05", Some(""));
    insert(&cassie, "n06", Some("alpha, beta... alpha-beta"));
    run(
        &cassie,
        "CREATE INDEX products_ft ON products USING fulltext (body)",
        vec![],
    );

    // Act
    let outcomes = AGREEMENT_QUERIES
        .iter()
        .map(|query| {
            (
                *query,
                indexed(&cassie, query),
                authoritative(&cassie, query),
            )
        })
        .collect::<Vec<_>>();

    // Assert
    for (query, persisted, row_wise) in outcomes {
        assert_eq!(persisted, row_wise, "{query:?}");
        assert!(
            !persisted.contains(&"n01".to_string()),
            "NULL never matches"
        );
        assert!(!persisted.contains(&"n05".to_string()));
    }
    assert_eq!(
        indexed(&cassie, "\"red shoes\""),
        expect(&["n02", "n03", "p02"])
    );
    assert_eq!(
        indexed(&cassie, "\"caf\u{e9} au\""),
        expect(&["n04", "p13"])
    );
}

#[test]
fn should_agree_inside_an_open_transaction_using_the_index_again_after_it() {
    // Arrange
    let cassie = corpus_cassie("");
    let session = cassie.create_session("tester", None);
    let exec = |sql: &str, params: Vec<Value>| {
        cassie
            .execute_sql(&session, sql, params)
            .expect("statement should execute")
            .rows()
            .to_vec()
    };
    let labels = |rows: Vec<Vec<Value>>, only_hits: bool| {
        let mut labels = rows
            .into_iter()
            .filter(|row| !only_hits || row[1] == Value::Bool(true))
            .map(|row| match &row[0] {
                Value::String(label) => label.clone(),
                _ => panic!("expected a text label"),
            })
            .collect::<Vec<_>>();
        labels.sort();
        labels
    };
    let phrase = vec![Value::String("\"red shoes\"".to_string())];
    let filtered = "SELECT label, search_score(body, $1, 'query') AS score FROM products \
                    WHERE search(body, $1, 'query')";
    let row_wise = "SELECT label, search(body, $1, 'query') FROM products";
    exec("BEGIN", vec![]);
    exec(
        "UPDATE products SET body = 'red shoes now' WHERE label = 'p03'",
        vec![],
    );
    exec("DELETE FROM products WHERE label = 'p02'", vec![]);
    exec(
        "INSERT INTO products (label, body) VALUES ('p99', 'new red shoes')",
        vec![],
    );
    let fallbacks = metric(&cassie, "row_scan_fallback_total");

    // Act
    let inside_filtered = labels(exec(filtered, phrase.clone()), false);
    let inside_row_wise = labels(exec(row_wise, phrase.clone()), true);
    let fallbacks_inside = metric(&cassie, "row_scan_fallback_total");
    exec("ROLLBACK", vec![]);
    let reads = metric(&cassie, "posting_reads_total");
    let after_rollback = labels(exec(filtered, phrase), false);

    // Assert
    assert_eq!(inside_filtered, expect(&["p03", "p99"]));
    assert_eq!(inside_row_wise, inside_filtered);
    assert!(
        fallbacks_inside > fallbacks,
        "overlay must use the fallback"
    );
    assert_eq!(after_rollback, expect(&["p02"]));
    assert!(metric(&cassie, "posting_reads_total") >= reads);
    assert_eq!(metric(&cassie, "row_scan_fallback_total"), fallbacks_inside);
}

#[test]
fn should_see_committed_updates_through_both_executions() {
    // Arrange
    let cassie = corpus_cassie("");
    run(
        &cassie,
        "UPDATE products SET body = 'shoes red' WHERE label = 'p02'",
        vec![],
    );
    run(&cassie, "DELETE FROM products WHERE label = 'p03'", vec![]);

    // Act
    let phrase = indexed(&cassie, "\"red shoes\"");
    let reversed = indexed(&cassie, "\"shoes red\"");

    // Assert
    assert_eq!(phrase, Vec::<String>::new());
    assert_eq!(reversed, expect(&["p02"]));
    assert_eq!(authoritative(&cassie, "\"red shoes\""), phrase);
    assert_eq!(authoritative(&cassie, "\"shoes red\""), reversed);
}

/// Terms within one edit of `abcde`: substitutions at every position and one
/// appended character.
fn near_abcde_terms() -> Vec<String> {
    let base: Vec<char> = "abcde".chars().collect();
    let mut terms = std::collections::BTreeSet::new();
    for position in 0..base.len() {
        for replacement in 'a'..='z' {
            let mut term = base.clone();
            term[position] = replacement;
            terms.insert(term.into_iter().collect::<String>());
        }
    }
    for appended in 'a'..='z' {
        terms.insert(format!("abcde{appended}"));
    }
    terms.into_iter().collect()
}

fn wide_cassie(terms: &[String], with_index: bool) -> Cassie {
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    run(&cassie, "CREATE TABLE wide (label TEXT, body TEXT)", vec![]);
    for term in terms {
        run(
            &cassie,
            "INSERT INTO wide (label, body) VALUES ($1, $1)",
            vec![Value::String(term.clone())],
        );
    }
    if with_index {
        run(
            &cassie,
            "CREATE INDEX wide_ft ON wide USING fulltext (body)",
            vec![],
        );
    }
    cassie
}

fn prefix_terms(prefix: &str, count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("{prefix}{index:04}"))
        .collect()
}

fn assert_limit(error: &cassie::app::CassieError, limit_name: &str) {
    assert_eq!(error.sql_state(), "54000");
    assert!(
        error.to_string().contains(limit_name),
        "the error names the {limit_name} limit"
    );
}

const FILTERED_WIDE: &str = "SELECT label, search_score(body, $1, 'query') AS score FROM wide \
                             WHERE search(body, $1, 'query')";
const ROW_WISE_WIDE: &str = "SELECT label, search(body, $1, 'query') FROM wide";

#[test]
fn should_return_the_program_limit_error_for_an_excessive_prefix_on_every_execution() {
    // Arrange
    let terms = prefix_terms("pre", MAX_EXPANSIONS_PER_ITEM + 1);
    let indexed_cassie = wide_cassie(&terms, true);
    let unindexed_cassie = wide_cassie(&terms, false);
    let excessive = vec![Value::String("pre*".to_string())];
    let within = vec![Value::String("pre000*".to_string())];

    // Act
    let outcomes = [
        (&indexed_cassie, FILTERED_WIDE),
        (&indexed_cassie, ROW_WISE_WIDE),
        (&unindexed_cassie, FILTERED_WIDE),
        (&unindexed_cassie, ROW_WISE_WIDE),
    ]
    .map(|(cassie, sql)| {
        let session = cassie.create_session("tester", None);
        let error = cassie
            .execute_sql(&session, sql, excessive.clone())
            .expect_err("the expansion limit applies");
        // The same session stays usable for a retry within the limit.
        let retry = cassie
            .execute_sql(&session, sql, within.clone())
            .expect("a retry on the same session succeeds");
        (error, retry.rows().len())
    });

    // Assert
    for (error, retry_rows) in &outcomes {
        assert_limit(error, "max_expansions_per_item");
        assert_ne!(*retry_rows, 0);
    }
}

#[test]
fn should_return_the_program_limit_error_for_an_excessive_typo_on_every_execution() {
    // Arrange
    let terms = near_abcde_terms();
    assert!(
        terms.len() > MAX_EXPANSIONS_PER_ITEM,
        "fixture must exceed the limit"
    );
    let indexed_cassie = wide_cassie(&terms, true);
    let unindexed_cassie = wide_cassie(&terms, false);
    let excessive = vec![Value::String("abcde~".to_string())];
    let within = vec![Value::String("abcde".to_string())];

    // Act
    let outcomes = [
        (&indexed_cassie, FILTERED_WIDE),
        (&indexed_cassie, ROW_WISE_WIDE),
        (&unindexed_cassie, FILTERED_WIDE),
        (&unindexed_cassie, ROW_WISE_WIDE),
    ]
    .map(|(cassie, sql)| {
        let session = cassie.create_session("tester", None);
        let error = cassie
            .execute_sql(&session, sql, excessive.clone())
            .expect_err("the expansion limit applies");
        // The same session stays usable for a retry within the limit.
        let retry = cassie
            .execute_sql(&session, sql, within.clone())
            .expect("a retry on the same session succeeds");
        (error, retry.rows().len())
    });

    // Assert
    for (error, retry_rows) in &outcomes {
        assert_limit(error, "max_expansions_per_item");
        assert_ne!(*retry_rows, 0);
    }
}

#[test]
fn should_return_the_program_limit_error_when_the_query_expands_past_the_total_limit() {
    // Arrange
    let per_item = 110;
    assert!(per_item <= MAX_EXPANSIONS_PER_ITEM);
    assert!(5 * per_item > MAX_EXPANDED_TERMS && 2 * per_item <= MAX_EXPANDED_TERMS);
    let mut terms = Vec::new();
    for prefix in ["aa", "bb", "cc", "dd", "ee"] {
        terms.extend(prefix_terms(prefix, per_item));
    }
    let cassie = wide_cassie(&terms, true);
    let session = cassie.create_session("tester", None);
    let query = vec![Value::String("aa* OR bb* OR cc* OR dd* OR ee*".to_string())];

    // Act
    let filtered = cassie
        .execute_sql(&session, FILTERED_WIDE, query.clone())
        .expect_err("the total limit applies to indexed execution");
    let row_wise = cassie
        .execute_sql(&session, ROW_WISE_WIDE, query)
        .expect_err("the total limit applies to authoritative execution");
    let retry = cassie
        .execute_sql(
            &session,
            FILTERED_WIDE,
            vec![Value::String("aa* OR bb*".to_string())],
        )
        .expect("a narrower query succeeds on the same session");

    // Assert
    assert_limit(&filtered, "max_expanded_terms");
    assert_limit(&row_wise, "max_expanded_terms");
    assert_eq!(retry.rows().len(), 2 * per_item);
}

#[test]
fn should_distinguish_opposite_token_orders_after_reopening_a_disk_backed_store() {
    // Arrange
    support::use_local_storage();
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let path = support::data_dir("fulltext_query_reopen");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let filtered = "SELECT label, search_score(body, $1, 'query') AS score FROM docs \
                    WHERE search(body, $1, 'query')";
    let row_wise = "SELECT label, search(body, $1, 'query') FROM docs";
    let phrase = |text: &str| vec![Value::String(text.to_string())];
    let hits = |rows: Vec<Vec<Value>>, only_true: bool| {
        rows.into_iter()
            .filter(|row| !only_true || row[1] == Value::Bool(true))
            .map(|row| row[0].clone())
            .collect::<Vec<_>>()
    };

    runtime.block_on(async {
        {
            let cassie = Cassie::new_with_data_dir(&path).expect("open store");
            cassie.startup().expect("start store");
            run(&cassie, "CREATE TABLE docs (label TEXT, body TEXT)", vec![]);
            for (label, body) in [
                ("forward", "ship the alpha beta"),
                ("reverse", "beta alpha"),
            ] {
                run(
                    &cassie,
                    "INSERT INTO docs (label, body) VALUES ($1, $2)",
                    vec![
                        Value::String(label.to_string()),
                        Value::String(body.to_string()),
                    ],
                );
            }
            run(
                &cassie,
                "CREATE INDEX docs_ft ON docs USING fulltext (body)",
                vec![],
            );
            let before = metric(&cassie, "posting_reads_total");

            // Act: indexed and authoritative execution before closing.
            let on_index = run(&cassie, filtered, phrase("\"alpha beta\""));
            let on_rows = run(&cassie, row_wise, phrase("\"alpha beta\""));

            // Assert
            assert_eq!(hits(on_index, false), vec![Value::String("forward".into())]);
            assert_eq!(hits(on_rows, true), vec![Value::String("forward".into())]);
            assert!(metric(&cassie, "posting_reads_total") > before);
        }

        // Act: reopen the same store and repeat both executions.
        let reopened = Cassie::new_with_data_dir(&path).expect("reopen store");
        reopened.startup().expect("restart store");
        let fallbacks = metric(&reopened, "row_scan_fallback_total");
        let reads = metric(&reopened, "posting_reads_total");
        let forward = run(&reopened, filtered, phrase("\"alpha beta\""));
        let reverse = run(&reopened, filtered, phrase("\"beta alpha\""));
        let indexed_reads = metric(&reopened, "posting_reads_total");
        let indexed_fallbacks = metric(&reopened, "row_scan_fallback_total");
        let forward_rows = run(&reopened, row_wise, phrase("\"alpha beta\""));
        let reverse_rows = run(&reopened, row_wise, phrase("\"beta alpha\""));

        // Assert
        assert_eq!(hits(forward, false), vec![Value::String("forward".into())]);
        assert_eq!(hits(reverse, false), vec![Value::String("reverse".into())]);
        assert_eq!(
            hits(forward_rows, true),
            vec![Value::String("forward".into())]
        );
        assert_eq!(
            hits(reverse_rows, true),
            vec![Value::String("reverse".into())]
        );
        assert!(
            indexed_reads > reads,
            "the reopened store answers from persisted postings"
        );
        assert_eq!(indexed_fallbacks, fallbacks);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_a_query_error_without_breaking_the_session() {
    // Arrange
    let cassie = corpus_cassie("");
    let session = cassie.create_session("tester", None);

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT label, search(body, $1, 'query') FROM products",
            vec![Value::String("(red".to_string())],
        )
        .expect_err("malformed query");
    let ok = cassie
        .execute_sql(
            &session,
            "SELECT label, search(body, $1, 'query') FROM products",
            vec![Value::String("red".to_string())],
        )
        .expect("the session remains usable");

    // Assert
    assert_eq!(error.sql_state(), "22023");
    assert_ne!(ok.rows().len(), 0);
}
