//! PostgreSQL 18 pattern and regular-expression behavior tracked by #799.

use cassie::app::CassieError;
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::QueryCancellationHandle;
use cassie::types::Value;

use crate::support_sql_fixture::{sql_fixture, sql_fixture_with_config, SqlFixture};

fn current_accounted_memory_bytes(fixture: &SqlFixture) -> u64 {
    fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"]
        .as_u64()
        .expect("current accounted query memory")
}

#[test]
fn should_match_ascii_values_with_ilike_patterns() {
    // Arrange
    let fixture = sql_fixture("issue-799-ilike", &[]);
    let cases = [
        ("SELECT 'Thomas' ILIKE 'thomas'", true),
        ("SELECT 'Thomas' ILIKE 'th%'", true),
        ("SELECT 'Thomas' ILIKE 't_omas'", true),
        ("SELECT 'Thomas' ILIKE 'tomas'", false),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, expected), result) in cases.iter().zip(results) {
        assert_eq!(
            result.expect("ILIKE pattern").rows(),
            &[vec![Value::Bool(*expected)]],
            "{sql}"
        );
    }
}

#[test]
fn should_preserve_null_results_for_ilike_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-799-ilike-null", &[]);

    // Act
    let result = fixture.execute("SELECT NULL ILIKE 'x%', NULL NOT ILIKE 'x%'");

    // Assert
    assert_eq!(
        result.expect("NULL ILIKE predicates").rows(),
        &[vec![Value::Null, Value::Null]]
    );
}

#[test]
fn should_bind_text_parameters_in_ilike_patterns() {
    // Arrange
    let fixture = sql_fixture("issue-799-ilike-parameter", &[]);

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT 'Alice' ILIKE $1 AS matched",
        vec![Value::String("a%".to_owned())],
    );

    // Assert
    assert_eq!(
        result.expect("bound ILIKE pattern").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_match_like_escape_characters_from_an_explicit_clause() {
    // Arrange
    let fixture = sql_fixture("issue-799-like-escape-clause", &[]);

    // Act
    let result = fixture.execute(
        "SELECT 'a_b' LIKE 'a!_b' ESCAPE '!', 'a%b' LIKE 'a!%b' ESCAPE '!', 'a!b' LIKE 'a!!b' ESCAPE '!'",
    );

    // Assert
    assert_eq!(
        result.expect("LIKE with explicit escape character").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_disable_like_escaping_for_an_empty_escape_clause() {
    // Arrange
    let fixture = sql_fixture("issue-799-like-empty-escape", &[]);

    // Act
    let result = fixture.execute("SELECT 'a!xb' LIKE 'a!_b' ESCAPE ''");

    // Assert
    assert_eq!(
        result.expect("LIKE with disabled escaping").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_evaluate_not_like_without_matching_null_values() {
    // Arrange
    let fixture = sql_fixture("issue-799-not-like", &[]);

    // Act
    let result = fixture.execute("SELECT 'abc' NOT LIKE 'a%', NULL NOT LIKE 'a%'");

    // Assert
    assert_eq!(
        result.expect("NOT LIKE predicates").rows(),
        &[vec![Value::Bool(false), Value::Null]]
    );
}

#[test]
fn should_evaluate_not_ilike_with_case_insensitive_matching() {
    // Arrange
    let fixture = sql_fixture("issue-799-not-ilike", &[]);

    // Act
    let result = fixture.execute("SELECT 'Alice' NOT ILIKE 'a%', 'Alice' NOT ILIKE 'z%'");

    // Assert
    assert_eq!(
        result.expect("NOT ILIKE predicates").rows(),
        &[vec![Value::Bool(false), Value::Bool(true)]]
    );
}

#[test]
fn should_match_similar_to_patterns_over_the_entire_string() {
    // Arrange
    let fixture = sql_fixture("issue-799-similar-to", &[]);

    // Act
    let result = fixture.execute(
        "SELECT 'abc' SIMILAR TO '%(b|d)%', 'abc' SIMILAR TO '(b|c)%', 'abc' SIMILAR TO 'b'",
    );

    // Assert
    assert_eq!(
        result.expect("SIMILAR TO full-string matches").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false)
        ]]
    );
}

#[test]
fn should_apply_escape_characters_to_similar_to_metacharacters() {
    // Arrange
    let fixture = sql_fixture("issue-799-similar-to-escape", &[]);

    // Act
    let result = fixture
        .execute("SELECT 'a%b' SIMILAR TO 'a!%b' ESCAPE '!', 'a_b' SIMILAR TO 'a!_b' ESCAPE '!'");

    // Assert
    assert_eq!(
        result.expect("SIMILAR TO escaped wildcards").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_evaluate_not_similar_to_with_sql_null_semantics() {
    // Arrange
    let fixture = sql_fixture("issue-799-not-similar-to", &[]);

    // Act
    let result = fixture.execute("SELECT 'abc' NOT SIMILAR TO 'a%', NULL NOT SIMILAR TO 'a%'");

    // Assert
    assert_eq!(
        result.expect("NOT SIMILAR TO predicates").rows(),
        &[vec![Value::Bool(false), Value::Null]]
    );
}

#[test]
fn should_apply_escape_clauses_to_not_similar_to() {
    // Arrange
    let fixture = sql_fixture("issue-799-not-similar-escape", &[]);

    // Act
    let result = fixture.execute("SELECT 'a%b' NOT SIMILAR TO 'a!%b' ESCAPE '!'");

    // Assert
    assert_eq!(
        result.expect("NOT SIMILAR TO escape clause").rows(),
        &[vec![Value::Bool(false)]]
    );
}

#[test]
fn should_accept_postgres_like_operator_spellings() {
    // Arrange
    let fixture = sql_fixture("issue-799-like-operator-spellings", &[]);

    // Act
    let result =
        fixture.execute("SELECT 'abc' ~~ 'a%', 'Abc' ~~* 'a%', 'abc' !~~ 'b%', 'Abc' !~~* 'b%'");

    // Assert
    assert_eq!(
        result.expect("PostgreSQL LIKE operator names").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_quantify_like_family_operators_over_text_arrays() {
    // Arrange
    let fixture = sql_fixture("issue-799-like-quantified-operators", &[]);

    // Act
    let result = fixture.execute(
        "SELECT 'alpha' LIKE ANY (ARRAY['b%', 'a%']), 'alpha' ILIKE ALL (ARRAY['A%', '%HA']), 'alpha' NOT LIKE ALL (ARRAY['b%', 'z%'])",
    );

    // Assert
    assert_eq!(
        result.expect("quantified LIKE-family operators").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_match_posix_patterns_without_implicit_anchors() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-operators", &[]);

    // Act
    let result = fixture
        .execute("SELECT 'abcd' ~ 'bc', 'abcd' ~ 'a.c', 'abcd' ~ '^(b|c)', 'abcd' !~ '^(b|c)'");

    // Assert
    assert_eq!(
        result.expect("POSIX regular-expression operators").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_filter_stored_text_rows_by_posix_regex_predicate() {
    // Arrange
    let fixture = sql_fixture(
        "issue-799-regex-table-predicate",
        &[
            "CREATE TABLE issue_799_regex_rows (body TEXT)",
            "INSERT INTO issue_799_regex_rows VALUES ('fat cat'), ('fat dog'), ('the cat')",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT body FROM issue_799_regex_rows WHERE body ~ 'cat' ORDER BY body");

    // Assert
    assert_eq!(
        result.expect("regex predicate over stored text").rows(),
        &[
            vec![Value::String("fat cat".to_owned())],
            vec![Value::String("the cat".to_owned())],
        ]
    );
}

#[test]
fn should_apply_case_insensitive_posix_regex_operator_spellings() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-case-insensitive", &[]);

    // Act
    let result = fixture.execute("SELECT 'Thomas' ~* 'T.*MA', 'Thomas' !~* 't.*ma'");

    // Assert
    assert_eq!(
        result.expect("case-insensitive regex operators").rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}

#[test]
fn should_apply_collation_dependent_unicode_regex_rules() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-unicode-collation", &[]);

    // Act
    let result = fixture.execute(
        "SELECT ('café' COLLATE \"C\") ~ '^[[:alpha:]]+$', ('café' COLLATE \"und-x-icu\") ~ '^[[:alpha:]]+$', ('café' COLLATE \"und-x-icu\") ~ '^\\w+$', ('café' COLLATE \"und-x-icu\") ~* '^CAFÉ$', ('é' COLLATE \"C\") ~* '^É$', ('é' COLLATE \"und-x-icu\") ~* '^É$'",
    );

    // Assert
    assert_eq!(
        result
            .expect("collation-aware Unicode regex matching")
            .rows(),
        &[vec![
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_preserve_null_results_for_regex_operators() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-null", &[]);

    // Act
    let result = fixture.execute("SELECT NULL ~ 'x', 'x' !~ NULL, NULL ~* 'x', 'x' !~* NULL");

    // Assert
    assert_eq!(
        result.expect("NULL regular-expression operators").rows(),
        &[vec![Value::Null, Value::Null, Value::Null, Value::Null]]
    );
}

#[test]
fn should_preserve_null_inputs_for_regex_functions() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-null-inputs", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_match(NULL, 'x')::TEXT, regexp_replace(NULL, 'x', 'y'), regexp_substr(NULL, 'x'), regexp_count(NULL, 'x')",
    );

    // Assert
    assert_eq!(
        result
            .expect("NULL regular-expression function inputs")
            .rows(),
        &[vec![Value::Null, Value::Null, Value::Null, Value::Null]]
    );
}

#[test]
fn should_quantify_regex_operators_over_text_arrays() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-quantified-operators", &[]);

    // Act
    let result = fixture
        .execute("SELECT 'alpha' ~ ANY (ARRAY['^b', 'ph']), 'alpha' !~ ALL (ARRAY['^b', '^z'])");

    // Assert
    assert_eq!(
        result.expect("quantified regex operators").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_match_postgres_are_backreferences_with_assertions() {
    // Arrange
    let fixture = sql_fixture("issue-799-are-backreference-assertions", &[]);
    let cases = [
        ("SELECT 'abab' ~ '^(ab)\\1$'", true),
        ("SELECT 'foobar' ~ 'foo(?=bar)'", true),
        ("SELECT 'foobar' ~ 'foo(?!baz)'", true),
        ("SELECT 'foobar' ~ '(?<=foo)bar'", true),
        ("SELECT 'foobar' ~ '(?<!baz)bar'", true),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, expected), result) in cases.iter().zip(results) {
        assert_eq!(
            result.expect("PostgreSQL ARE constraint").rows(),
            &[vec![Value::Bool(*expected)]],
            "{sql}"
        );
    }
}

#[test]
fn should_match_postgres_are_word_constraints_with_character_classes() {
    // Arrange
    let fixture = sql_fixture("issue-799-are-word-classes", &[]);
    let cases = [
        ("SELECT 'cat' ~ '\\mcat\\M'", true),
        ("SELECT 'cat' ~ '\\ycat\\y'", true),
        ("SELECT 'foobar' ~ 'foo\\Ybar'", true),
        ("SELECT 'cat' ~ '\\Acat\\Z'", true),
        ("SELECT 'alpha' ~ '^[[:alpha:]]+$'", true),
        ("SELECT 'alpha' ~ '^[[=a=]]lph[[:alpha:]]$'", true),
        ("SELECT 'a' ~ '^[[.a.]]$'", true),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, expected), result) in cases.iter().zip(results) {
        assert_eq!(
            result
                .expect("PostgreSQL ARE word or class expression")
                .rows(),
            &[vec![Value::Bool(*expected)]],
            "{sql}"
        );
    }
}

#[test]
fn should_match_postgres_are_option_director_forms() {
    // Arrange
    let fixture = sql_fixture("issue-799-are-options-directors", &[]);
    let cases = [
        ("SELECT 'ABC' ~ '(?i)^abc$'", true),
        ("SELECT 'alpha' ~ '(?x)^ a l p h a $'", true),
        ("SELECT 'a.b' ~ '***=a.b'", true),
        ("SELECT 'alpha' ~ '***:^alpha$'", true),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, expected), result) in cases.iter().zip(results) {
        assert_eq!(
            result.expect("PostgreSQL ARE option or director").rows(),
            &[vec![Value::Bool(*expected)]],
            "{sql}"
        );
    }
}

#[test]
fn should_match_posix_bre_ere_modes() {
    // Arrange
    let fixture = sql_fixture("issue-799-basic-extended-regex-modes", &[]);

    // Act
    let result = fixture
        .execute("SELECT regexp_like('a+b', '^a+b$', 'b'), regexp_like('aaab', '^a+b$', 'e')");

    // Assert
    assert_eq!(
        result.expect("BRE and ERE modes").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_apply_postgres_regex_quoted_case_flags() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-quoted-flags", &[]);

    // Act
    let result = fixture
        .execute("SELECT regexp_like('^a+$', '^a+$', 'q'), regexp_like('Thomas', 'thomas', 'c')");

    // Assert
    assert_eq!(
        result
            .expect("quoted and case-sensitive regex flags")
            .rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}

#[test]
fn should_apply_postgres_regex_newline_sensitive_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-newline-sensitive", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('a\nb', 'a.b', 'n')");

    // Assert
    assert_eq!(
        result.expect("newline-sensitive regex flag").rows(),
        &[vec![Value::Bool(false)]]
    );
}

#[test]
fn should_apply_postgres_regex_multiline_alias_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-multiline-alias", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('a\nb', 'a.b', 'm')");

    // Assert
    assert_eq!(
        result.expect("historical multiline regex flag").rows(),
        &[vec![Value::Bool(false)]]
    );
}

#[test]
fn should_apply_postgres_regex_non_newline_sensitive_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-non-newline-sensitive", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('a\nb', 'a.b', 's')");

    // Assert
    assert_eq!(
        result.expect("non-newline-sensitive regex flag").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_apply_postgres_regex_partial_newline_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-partial-newline", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('a\nb', '^b$', 'p')");

    // Assert
    assert_eq!(
        result.expect("partial newline-sensitive regex flag").rows(),
        &[vec![Value::Bool(false)]]
    );
}

#[test]
fn should_apply_postgres_regex_inverse_partial_newline_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-inverse-partial-newline", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('a\nb', '^b$', 'w')");

    // Assert
    assert_eq!(
        result
            .expect("inverse partial newline-sensitive regex flag")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_apply_postgres_regex_tight_syntax_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-tight-syntax", &[]);

    // Act
    let result =
        fixture.execute("SELECT regexp_like('a b', 'a b', 't'), regexp_like('ab', 'a b', 't')");

    // Assert
    assert_eq!(
        result.expect("tight-syntax regex flag").rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}

#[test]
fn should_apply_postgres_regex_expanded_syntax_function_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-expanded-flag", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('alpha', '^ a l p h a $', 'x')");

    // Assert
    assert_eq!(
        result.expect("expanded-syntax regex function flag").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_count_matches_with_postgres_regex_start_options() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_count('ABCABCAXYaxy', 'A.'), regexp_count('ABCABCAXYaxy', 'A.', 1, 'i'), regexp_count('ABCABCAXYaxy', 'A.', 4, 'i')",
    );

    // Assert
    assert_eq!(
        result.expect("regexp_count").rows(),
        &[vec![Value::Int64(3), Value::Int64(4), Value::Int64(3)]]
    );
}

#[test]
fn should_return_match_positions_for_selected_regex_occurrences() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-instr", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_instr('number of your street, town zip, FR', '[^,]+', 1, 2), regexp_instr('number of your street, town zip, FR', '[^,]+', 1, 2, 1), regexp_instr('ABCDEF', '(A)(BC)', 1, 1, 0, '', 2)",
    );

    // Assert
    assert_eq!(
        result.expect("regexp_instr").rows(),
        &[vec![Value::Int64(23), Value::Int64(32), Value::Int64(2)]]
    );
}

#[test]
fn should_count_regex_offsets_in_characters_for_multibyte_text() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-character-offsets", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_instr('Aé🙂Z', '🙂'), regexp_substr('Aé🙂Z', '.', 2, 2), regexp_replace('Aé🙂Z', '.', 'x', 2, 2, '')",
    );

    // Assert
    assert_eq!(
        result
            .expect("regex character offsets over UTF-8 text")
            .rows(),
        &[vec![
            Value::Int64(3),
            Value::String("🙂".to_owned()),
            Value::String("AéxZ".to_owned()),
        ]]
    );
}

#[test]
fn should_return_captured_groups_from_the_first_regex_match() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-match", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_match('foobarbequebaz', '(bar)(beque)')::TEXT AS captures, regexp_match('foobaz', '(bar)')::TEXT AS absent",
    );

    // Assert
    assert_eq!(
        result.expect("regexp_match captures").rows(),
        &[vec![Value::String("{bar,beque}".to_owned()), Value::Null,]]
    );
}

#[test]
fn should_return_each_regex_match_when_global_matching_is_requested() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-matches-global", &[]);

    // Act
    let result = fixture.execute(
        "SELECT matches::TEXT FROM regexp_matches('foobarbequebazilbarfbonk', '(b[^b]+)(b[^b]+)', 'g') AS matches",
    );

    // Assert
    assert_eq!(
        result.expect("global regexp_matches").rows(),
        &[
            vec![Value::String("{bar,beque}".to_owned())],
            vec![Value::String("{bazil,barf}".to_owned())],
        ]
    );
}

#[test]
fn should_return_the_selected_regex_substring_capture() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-substr", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_substr('ABCDEFGHI', '(c..)(...)', 1, 1, 'i', 2), regexp_substr('abc', 'z')",
    );

    // Assert
    assert_eq!(
        result.expect("regexp_substr").rows(),
        &[vec![Value::String("FGH".to_owned()), Value::Null]]
    );
}

#[test]
fn should_split_text_into_regex_collections() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-split", &[]);

    // Act
    let array = fixture.execute("SELECT regexp_split_to_array('one two three', '\\s+')::TEXT");
    let rows = fixture.execute(
        "SELECT part FROM regexp_split_to_table('one two three', '\\s+') AS part ORDER BY part",
    );

    // Assert
    assert_eq!(
        array.expect("regexp_split_to_array").rows(),
        &[vec![Value::String("{one,two,three}".to_owned())]]
    );
    assert_eq!(
        rows.expect("regexp_split_to_table").rows(),
        &[
            vec![Value::String("one".to_owned())],
            vec![Value::String("three".to_owned())],
            vec![Value::String("two".to_owned())],
        ]
    );
}

#[test]
fn should_enforce_regex_split_result_cap_with_cleanup() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.max_result_rows = 2;
    let fixture = sql_fixture_with_config("issue-799-regex-split-result-limit", &[], config);
    let memory_before = current_accounted_memory_bytes(&fixture);

    // Act
    let result = fixture.execute("SELECT part FROM regexp_split_to_table('a,b,c', ',') AS part");

    // Assert
    assert!(
        matches!(result, Err(CassieError::ResourceLimit(_))),
        "regex expansion beyond max_result_rows must fail as a resource limit: {result:?}"
    );
    assert_eq!(current_accounted_memory_bytes(&fixture), memory_before);
    assert_eq!(
        fixture
            .execute("SELECT 1::BIGINT")
            .expect("query after bounded regex expansion")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_cancel_recursive_regex_work_with_cleanup() {
    // Arrange
    let mut config = CassieRuntimeConfig::default();
    config.limits.cte_recursion_depth = 1_000_000;
    config.limits.query_memory_budget_bytes = 1024 * 1024 * 1024;
    let fixture = std::sync::Arc::new(sql_fixture_with_config(
        "issue-799-regex-active-cancellation",
        &[],
        config,
    ));
    let memory_before = current_accounted_memory_bytes(&fixture);
    let cancellation = QueryCancellationHandle::new();
    let query_cancellation = cancellation.clone();
    let query_fixture = std::sync::Arc::clone(&fixture);
    let query = std::thread::spawn(move || {
        query_fixture.cassie.execute_sql_with_cancellation(
            &query_fixture.session,
            "WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM seq WHERE n < 1000000 AND n::TEXT ~ '^[0-9]+$') SELECT MAX(n) FROM seq",
            vec![],
            &query_cancellation,
        )
    });
    std::thread::sleep(std::time::Duration::from_millis(25));

    // Act
    cancellation.cancel();
    let error = query
        .join()
        .expect("regex query thread")
        .expect_err("active regex query should be cancelled");

    // Assert
    assert!(matches!(error, CassieError::QueryCancelled));
    assert_eq!(current_accounted_memory_bytes(&fixture), memory_before);
    assert_eq!(
        fixture
            .execute("SELECT 1::BIGINT")
            .expect("query after regex cancellation")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_replace_the_first_or_all_regex_matches_with_captured_text() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_replace('foobarbaz', 'b(..)', 'X\\1Y'), regexp_replace('foobarbaz', 'b(..)', 'X\\1Y', 'g'), regexp_replace('A PostgreSQL function', 'a|e|i|o|u', 'X', 1, 0, 'i')",
    );

    // Assert
    assert_eq!(
        result.expect("regexp_replace variants").rows(),
        &[vec![
            Value::String("fooXarYbaz".to_owned()),
            Value::String("fooXarYXazY".to_owned()),
            Value::String("X PXstgrXSQL fXnctXXn".to_owned()),
        ]]
    );
}

#[test]
fn should_extract_substrings_from_postgres_pattern_forms() {
    // Arrange
    let fixture = sql_fixture("issue-799-substring-patterns", &[]);

    // Act
    let result = fixture.execute(
        "SELECT substring('foobar' FROM 'o(.)b'), substring('foobar' SIMILAR '%#\"o_b#\"%' ESCAPE '#'), substring('foobar', '%#\"o_b#\"%', '#')",
    );

    // Assert
    assert_eq!(
        result.expect("pattern substring forms").rows(),
        &[vec![
            Value::String("o".to_owned()),
            Value::String("oob".to_owned()),
            Value::String("oob".to_owned()),
        ]]
    );
}

#[test]
fn should_accept_named_arguments_for_regexp_match() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-match-named", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_match(string => 'foobarbequebaz', pattern => '(bar)(beque)')::TEXT",
    );

    // Assert
    assert_eq!(
        result.expect("named regexp_match arguments").rows(),
        &[vec![Value::String("{bar,beque}".to_owned())]]
    );
}

#[test]
fn should_accept_named_arguments_for_regexp_matches() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-matches-named", &[]);

    // Act
    let result = fixture.execute(
        "SELECT matches::TEXT FROM regexp_matches(string => 'ab ac', pattern => 'a.', flags => 'g') AS matches",
    );

    // Assert
    assert_eq!(
        result.expect("named regexp_matches arguments").rows(),
        &[
            vec![Value::String("{ab}".to_owned())],
            vec![Value::String("{ac}".to_owned())],
        ]
    );
}

#[test]
fn should_accept_named_arguments_for_regexp_like() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-like-named", &[]);

    // Act
    let result = fixture
        .execute("SELECT regexp_like(string => 'Hello World', pattern => 'world', flags => 'i')");

    // Assert
    assert_eq!(
        result.expect("named regexp_like arguments").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_accept_named_arguments_for_regexp_replace() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-replace-named", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_replace(string => 'A PostgreSQL function', pattern => 'a|e|i|o|u', replacement => 'X', start => 1, \"N\" => 3, flags => 'i')",
    );

    // Assert
    assert_eq!(
        result.expect("named regexp_replace arguments").rows(),
        &[vec![Value::String("A PostgrXSQL function".to_owned())]]
    );
}

#[test]
fn should_accept_named_arguments_for_regexp_count() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-named", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_count(string => 'ABCABCAXYaxy', pattern => 'A.', start => 1, flags => 'i')",
    );

    // Assert
    assert_eq!(
        result.expect("named regexp_count arguments").rows(),
        &[vec![Value::Int64(4)]]
    );
}

#[test]
fn should_accept_named_arguments_for_regexp_instr() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-instr-named", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_instr(string => 'ABCDEFGHI', pattern => '(c..)(...)', start => 1, \"N\" => 1, endoption => 0, flags => 'i', subexpr => 2)",
    );

    // Assert
    assert_eq!(
        result.expect("named regexp_instr arguments").rows(),
        &[vec![Value::Int64(6)]]
    );
}

#[test]
fn should_accept_named_arguments_for_regexp_substr() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-substr-named", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_substr(string => 'ABCDEFGHI', pattern => '(c..)(...)', start => 1, \"N\" => 1, flags => 'i', subexpr => 2)",
    );

    // Assert
    assert_eq!(
        result.expect("named regexp_substr arguments").rows(),
        &[vec![Value::String("FGH".to_owned())]]
    );
}

#[test]
fn should_accept_named_arguments_for_regex_split_functions() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-split-named", &[]);

    // Act
    let array = fixture
        .execute("SELECT regexp_split_to_array(string => 'one two', pattern => '\\s+')::TEXT");
    let rows = fixture.execute(
        "SELECT part FROM regexp_split_to_table(string => 'one two', pattern => '\\s+') AS part ORDER BY part",
    );

    // Assert
    assert_eq!(
        array.expect("named regexp_split_to_array arguments").rows(),
        &[vec![Value::String("{one,two}".to_owned())]]
    );
    assert_eq!(
        rows.expect("named regexp_split_to_table arguments").rows(),
        &[
            vec![Value::String("one".to_owned())],
            vec![Value::String("two".to_owned())],
        ]
    );
}
