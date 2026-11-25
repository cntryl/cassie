use super::fulltext_query_support::{
    authoritative, authoritative_scores, corpus_cassie, corpus_table, expect, indexed,
    indexed_top_k, insert, metric, run, run_error,
};
use cassie::types::Value;

/// The expected IDs of one prepared query beside what each execution returned.
struct Outcome {
    query: &'static str,
    expected: Vec<String>,
    indexed: Vec<String>,
    authoritative: Vec<String>,
}

/// Runs each prepared query on indexed and authoritative execution.
fn evaluate_cases(cassie: &cassie::app::Cassie, cases: &[(&'static str, &[&str])]) -> Vec<Outcome> {
    cases
        .iter()
        .map(|(query, expected)| Outcome {
            query,
            expected: expect(expected),
            indexed: indexed(cassie, query),
            authoritative: authoritative(cassie, query),
        })
        .collect()
}

fn assert_cases(outcomes: &[Outcome]) {
    for outcome in outcomes {
        assert_eq!(
            outcome.indexed, outcome.expected,
            "indexed execution of {:?}",
            outcome.query
        );
        assert_eq!(
            outcome.authoritative, outcome.expected,
            "authoritative execution of {:?}",
            outcome.query
        );
    }
}

#[test]
fn should_distinguish_adjacent_from_separated_terms_given_a_quoted_phrase() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let outcomes = evaluate_cases(
        &cassie,
        &[
            ("\"red shoes\"", &["p02"]),
            ("\"shoes red\"", &["p03"]),
            ("\"red lightweight shoes\"", &["p04"]),
            ("red shoes", &["p01", "p02", "p03", "p04"]),
        ],
    );

    // Assert
    assert_cases(&outcomes);
}

#[test]
fn should_allow_ordered_slack_given_phrase_slop() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let outcomes = evaluate_cases(
        &cassie,
        &[
            ("\"red shoes\"~1", &["p01", "p02", "p04"]),
            ("\"red shoes\"~0", &["p02"]),
            ("\"shoes red\"~3", &["p03"]),
        ],
    );

    // Assert
    assert_cases(&outcomes);
}

#[test]
fn should_preserve_stop_word_gaps_given_phrases_and_hyphenated_words() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let outcomes = evaluate_cases(
        &cassie,
        &[
            ("\"alpha beta\"", &["p10"]),
            ("\"alpha the beta\"", &["p12"]),
            ("alpha-beta", &["p10"]),
            ("alpha beta", &["p10", "p11", "p12"]),
            ("the red shoes", &["p01", "p02", "p03", "p04"]),
        ],
    );

    // Assert
    assert_cases(&outcomes);
}

#[test]
fn should_separate_prefix_collisions_given_wildcard_terms() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let outcomes = evaluate_cases(
        &cassie,
        &[
            ("shoes*", &["p01", "p02", "p03", "p04", "p05"]),
            ("shoe*", &["p01", "p02", "p03", "p04", "p05", "p06", "p07"]),
            ("shoel*", &["p07"]),
            ("red sho*", &["p01", "p02", "p03", "p04", "p07"]),
            ("zzz*", &[]),
        ],
    );

    // Assert
    assert_cases(&outcomes);
}

#[test]
fn should_apply_required_excluded_terms_given_boolean_operators() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let outcomes = evaluate_cases(
        &cassie,
        &[
            ("red -running", &["p02", "p03", "p07"]),
            ("shoes +red NOT sale", &["p01", "p03", "p04"]),
            ("running OR care", &["p01", "p04", "p05", "p06"]),
            ("(red OR blue) running", &["p01", "p04", "p05"]),
            ("\"red shoes\" -sale", &[]),
            ("shoes -(red OR blue)", &[]),
        ],
    );

    // Assert
    assert_cases(&outcomes);
}

#[test]
fn should_match_single_edit_misspellings_given_typo_terms() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let outcomes = evaluate_cases(
        &cassie,
        &[
            ("colour~", &["p08", "p09"]),
            ("colr~", &["p09"]),
            ("shoos~", &["p01", "p02", "p03", "p04", "p05"]),
            ("runing~", &["p01", "p04", "p05"]),
            ("lightwieght~", &[]),
        ],
    );

    // Assert
    assert_cases(&outcomes);
}

#[test]
fn should_match_unicode_text_given_punctuation() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let outcomes = evaluate_cases(
        &cassie,
        &[
            ("\"au lait\"", &["p13"]),
            ("caf\u{e9}", &["p13"]),
            ("caf*", &["p13"]),
            ("na\u{ef}ve ideas", &["p13"]),
            ("\"lait na\u{ef}ve\"", &["p13"]),
        ],
    );

    // Assert
    assert_cases(&outcomes);
}

#[test]
fn should_fold_accents_for_prefix_words_given_accent_folding() {
    // Arrange
    let cassie = corpus_cassie(" WITH (accent_folding = 'true')");

    // Act
    let outcomes = evaluate_cases(
        &cassie,
        &[
            ("cafe*", &["p13"]),
            ("CAF\u{c9}", &["p13"]),
            ("naive~", &["p13"]),
        ],
    );

    // Assert
    assert_cases(&outcomes);
}

#[test]
fn should_match_nothing_given_blank_or_stop_word_only_queries() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let outcomes = evaluate_cases(
        &cassie,
        &[("", &[]), ("   ", &[]), ("the", &[]), ("the -red", &[])],
    );

    // Assert
    assert_cases(&outcomes);
}

#[test]
fn should_keep_the_two_argument_form_identical_to_the_terms_syntax() {
    // Arrange
    let cassie = corpus_cassie("");
    let query = Value::String("red shoes".to_string());

    // Act
    let two_argument = run(
        &cassie,
        "SELECT label, search(body, $1), search_score(body, $1) FROM products ORDER BY label",
        vec![query.clone()],
    );
    let terms = run(
        &cassie,
        "SELECT label, search(body, $1, 'terms'), search_score(body, $1, 'terms') \
         FROM products ORDER BY label",
        vec![query.clone()],
    );
    let syntax_parameter = run(
        &cassie,
        "SELECT label, search(body, $1, $2), search_score(body, $1, $2) \
         FROM products ORDER BY label",
        vec![query.clone(), Value::String("terms".to_string())],
    );
    let indexed_two = run(
        &cassie,
        "SELECT label FROM products WHERE search(body, $1) ORDER BY label",
        vec![query],
    );

    // Assert
    assert_eq!(two_argument, terms);
    assert_eq!(two_argument, syntax_parameter);
    let matched = indexed_two
        .into_iter()
        .map(|row| row[0].clone())
        .collect::<Vec<_>>();
    let any_token = ["p01", "p02", "p03", "p04", "p05", "p07"]
        .map(|label| Value::String(label.to_string()))
        .to_vec();
    assert_eq!(matched, any_token);
}

#[test]
fn should_accept_the_syntax_selector_as_a_bound_parameter() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let rows = run(
        &cassie,
        "SELECT label, search_score(body, $1, $2) AS score FROM products \
         WHERE search(body, $1, $2)",
        vec![
            Value::String("\"red shoes\"".to_string()),
            Value::String("query".to_string()),
        ],
    );

    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], Value::String("p02".to_string()));
}

#[test]
fn should_return_invalid_parameter_errors_given_malformed_queries() {
    // Arrange
    let cassie = corpus_cassie("");
    let malformed = [
        "\"unterminated",
        "(red",
        "red)",
        "red OR",
        "-red",
        "red OR -blue",
        "colr~2",
        "r*",
        "abc~",
        "foo-bar*",
        "\\",
    ];

    // Act
    let states = malformed.map(|query| {
        let filtered = run_error(
            &cassie,
            "SELECT label, search_score(body, $1, 'query') AS score FROM products \
             WHERE search(body, $1, 'query')",
            vec![Value::String(query.to_string())],
        );
        let row_wise = run_error(
            &cassie,
            "SELECT label, search(body, $1, 'query') FROM products",
            vec![Value::String(query.to_string())],
        );
        (query, filtered.sql_state(), row_wise.sql_state())
    });

    // Assert
    for (query, indexed_state, authoritative_state) in states {
        assert_eq!(indexed_state, "22023", "indexed {query:?}");
        assert_eq!(authoritative_state, "22023", "authoritative {query:?}");
    }
}

#[test]
fn should_reject_an_unknown_syntax_selector_with_an_invalid_parameter_error() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let literal = run_error(
        &cassie,
        "SELECT label FROM products WHERE search(body, 'red', 'fuzzy')",
        vec![],
    );
    let parameter = run_error(
        &cassie,
        "SELECT label, search_score(body, $1, $2) AS score FROM products \
         WHERE search(body, $1, $2)",
        vec![
            Value::String("red".to_string()),
            Value::String("QUERY".to_string()),
        ],
    );
    let null_selector = run_error(
        &cassie,
        "SELECT label, search(body, 'red', NULL) FROM products",
        vec![],
    );

    // Assert
    assert_eq!(literal.sql_state(), "22023");
    assert_eq!(parameter.sql_state(), "22023");
    assert_eq!(null_selector.sql_state(), "22023");
}

#[test]
fn should_reject_queries_over_the_declared_structural_limits() {
    // Arrange
    let cassie = corpus_cassie("");
    let too_long = "a".repeat(cassie::search::query::limits::MAX_QUERY_BYTES + 1);
    let too_many_items = vec!["red"; cassie::search::query::limits::MAX_QUERY_ITEMS + 1].join(" ");
    let too_deep = format!(
        "{}red{}",
        "(".repeat(cassie::search::query::limits::MAX_QUERY_DEPTH + 1),
        ")".repeat(cassie::search::query::limits::MAX_QUERY_DEPTH + 1)
    );
    let too_much_slop = "\"red shoes\"~99".to_string();

    // Act
    let states = [too_long, too_many_items, too_deep, too_much_slop].map(|query| {
        run_error(
            &cassie,
            "SELECT label, search(body, $1, 'query') FROM products",
            vec![Value::String(query)],
        )
        .sql_state()
    });

    // Assert
    assert_eq!(states, ["22023"; 4]);
}

#[test]
fn should_score_only_satisfied_positive_terms_with_corpus_wide_statistics() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let phrase = indexed_top_k(&cassie, "\"red shoes\"");
    let any_token = run(
        &cassie,
        "SELECT label, search_score(body, 'red shoes', 'terms') AS score FROM products \
         WHERE search(body, 'red shoes', 'terms') ORDER BY score DESC LIMIT 50",
        vec![],
    );
    let alternatives = indexed_top_k(&cassie, "red OR blue");

    // Assert
    assert_eq!(phrase.len(), 1);
    assert_eq!(phrase[0].0, "p02");
    let terms_score = any_token
        .iter()
        .find_map(|row| match (&row[0], &row[1]) {
            (Value::String(label), Value::Float64(score)) if label == "p02" => Some(*score),
            _ => None,
        })
        .expect("p02 matches the any-token query");
    assert!(
        (phrase[0].1 - terms_score).abs() < 1e-9,
        "a matched phrase scores the BM25 of its terms"
    );
    let single = alternatives
        .iter()
        .find(|(label, _)| label == "p05")
        .expect("p05 contains blue");
    let both = alternatives
        .iter()
        .find(|(label, _)| label == "p02")
        .expect("p02 contains red");
    assert!(single.1 > 0.0 && both.1 > 0.0);
}

#[test]
fn should_match_scores_given_indexed_execution_versus_authoritative_execution() {
    // Arrange
    let cassie = corpus_cassie("");

    // Act
    let compared = [
        "\"red shoes\"~1",
        "shoe* -sale",
        "colour~ OR poster",
        "(red OR blue) running",
    ]
    .map(|query| {
        let mut persisted = indexed_top_k(&cassie, query);
        persisted.sort_by(|left, right| left.0.cmp(&right.0));
        (query, persisted, authoritative_scores(&cassie, query))
    });

    // Assert
    for (query, persisted, row_wise) in compared {
        assert_eq!(persisted.len(), row_wise.len(), "{query:?}");
        for (left, right) in persisted.iter().zip(&row_wise) {
            assert_eq!(left.0, right.0, "{query:?}");
            assert!(
                (left.1 - right.1).abs() < 1e-9,
                "{query:?}: {} vs {}",
                left.1,
                right.1
            );
        }
    }
}

#[test]
fn should_highlight_only_tokens_that_satisfied_positive_items() {
    // Arrange
    let cassie = corpus_table();
    insert(&cassie, "extra", Some("red hat red shoes shoes"));
    run(
        &cassie,
        "CREATE INDEX products_ft ON products USING fulltext (body)",
        vec![],
    );
    let phrase = Value::String("\"red shoes\"".to_string());

    // Act
    let authoritative_snippets = run(
        &cassie,
        "SELECT label, snippet(body, $1, 'query') FROM products \
         WHERE label IN ('extra', 'p02', 'p04')",
        vec![phrase.clone()],
    );
    let indexed_snippets = run(
        &cassie,
        "SELECT label, snippet(body, $1, 'query') AS excerpt, \
         search_score(body, $1, 'query') AS score FROM products WHERE search(body, $1, 'query')",
        vec![phrase],
    );
    let exclusion = run(
        &cassie,
        "SELECT snippet(body, 'shoes -sale', 'query') FROM products WHERE label = 'p01'",
        vec![],
    );

    // Assert
    let find = |rows: &[Vec<Value>], label: &str| {
        rows.iter()
            .find(|row| row[0] == Value::String(label.to_string()))
            .map(|row| row[1].clone())
    };
    let extra = Value::String("red hat <mark>red</mark> <mark>shoes</mark> shoes".to_string());
    assert_eq!(find(&authoritative_snippets, "extra"), Some(extra.clone()));
    assert_eq!(find(&indexed_snippets, "extra"), Some(extra));
    assert_eq!(
        find(&authoritative_snippets, "p02"),
        Some(Value::String(
            "<mark>red</mark> <mark>shoes</mark> on sale".to_string()
        ))
    );
    assert_eq!(
        find(&authoritative_snippets, "p04"),
        Some(Value::String("running red lightweight shoes".to_string()))
    );
    assert_eq!(
        exclusion[0][0],
        Value::String("red running <mark>shoes</mark> for trail".to_string())
    );
}

#[test]
fn should_use_the_persisted_index_given_phrase_prefix_and_typo_queries() {
    // Arrange
    let cassie = corpus_cassie("");
    let posting_reads = metric(&cassie, "posting_reads_total");
    let fallbacks = metric(&cassie, "row_scan_fallback_total");

    // Act
    let phrase = indexed(&cassie, "\"red shoes\"");
    let prefix = indexed(&cassie, "shoel*");
    let typo = indexed(&cassie, "colr~");
    let top_k = indexed_top_k(&cassie, "\"red shoes\" OR care");

    // Assert
    assert_eq!(phrase, expect(&["p02"]));
    assert_eq!(prefix, expect(&["p07"]));
    assert_eq!(typo, expect(&["p09"]));
    assert_eq!(top_k.len(), 2);
    assert!(metric(&cassie, "posting_reads_total") > posting_reads);
    assert_eq!(metric(&cassie, "row_scan_fallback_total"), fallbacks);
}

#[test]
fn should_rank_the_phrase_match_first_given_hybrid_scoring() {
    // Arrange
    let cassie = cassie_with_vectors();
    run(
        &cassie,
        "CREATE TABLE hybrid_docs (label TEXT, body TEXT, embedding VECTOR(2))",
        vec![],
    );
    for (label, body) in [
        ("near", "red shoes on sale"),
        ("far", "shoes red"),
        ("none", "blue socks"),
    ] {
        run(
            &cassie,
            "INSERT INTO hybrid_docs (label, body, embedding) VALUES ($1, $2, '[1,0]')",
            vec![
                Value::String(label.to_string()),
                Value::String(body.to_string()),
            ],
        );
    }
    run(
        &cassie,
        "CREATE INDEX hybrid_docs_ft ON hybrid_docs USING fulltext (body)",
        vec![],
    );

    // Act
    let rows = run(
        &cassie,
        "SELECT label, hybrid_score(search_score(body, $1, 'query'), \
         vector_score(embedding, '[1,0]')) AS hs FROM hybrid_docs ORDER BY hs DESC LIMIT 3",
        vec![Value::String("\"red shoes\"".to_string())],
    );

    // Assert
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0][0], Value::String("near".to_string()));
    let scores = rows
        .iter()
        .map(|row| match row[1] {
            Value::Float64(score) => score,
            _ => panic!("expected a float hybrid score"),
        })
        .collect::<Vec<_>>();
    assert!(
        scores[0] > scores[1] && scores[1] >= scores[2],
        "{scores:?}"
    );
}

fn cassie_with_vectors() -> cassie::app::Cassie {
    let cassie = super::support_sql::new_in_memory_cassie().expect("create Cassie");
    super::support_sql::install_vector_extension(&cassie);
    cassie.startup().expect("start Cassie");
    cassie
}
