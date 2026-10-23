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
fn should_apply_unicode_simple_case_mapping_to_ilike_literals() {
    // Arrange
    let fixture = sql_fixture("issue-799-ilike-unicode-simple-case-mapping", &[]);

    // Act
    let result = fixture.execute(
        "SELECT 'é' ILIKE 'É', 'İ' ILIKE 'i', 'ẞ' ILIKE 'ß', 'Σ' ILIKE 'σ', 'Σ' ILIKE 'ς', 'İ' ILIKE 'i̇'",
    );

    // Assert
    assert_eq!(
        result.expect("Unicode ILIKE simple case mapping").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
        ]]
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
fn should_apply_escape_clauses_to_ilike_predicates() {
    // Arrange
    let fixture = sql_fixture("issue-799-ilike-escape-clause", &[]);

    // Act
    let result = fixture.execute(
        "SELECT 'A_b' ILIKE 'a!_B' ESCAPE '!', 'A%b' ILIKE 'a!%B' ESCAPE '!', '_A' ILIKE 'A_a' ESCAPE 'A'",
    );

    // Assert
    assert_eq!(
        result.expect("ILIKE with explicit escape character").rows(),
        &[vec![Value::Bool(true), Value::Bool(true), Value::Bool(true)]]
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
fn should_ignore_nonexpression_regex_tokens() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-operator-token-boundaries", &[]);

    // Act
    let result = fixture.execute(
        "SELECT 'a~b' = 'a~b', 'alpha' /* ~ ignored */ ~ 'ph'",
    );

    // Assert
    assert_eq!(
        result.expect("regex tokens in strings and comments").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_regex_comparison_precedence() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-operator-comparison-precedence", &[]);

    // Act
    let result = fixture.execute("SELECT 'alpha' ~ 'ph' = true");

    // Assert
    assert_eq!(
        result.expect("regex comparison precedence").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_report_2201b_for_invalid_regex_operator_patterns() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-operator-invalid-pattern", &[]);

    // Act
    let error = fixture
        .execute("SELECT 'x' ~ '['")
        .expect_err("invalid regular-expression operator pattern should fail");

    // Assert
    assert!(matches!(
        error,
        CassieError::Diagnostic(ref diagnostic) if diagnostic.sql_state == "2201B"
    ));
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
fn should_match_postgres_regex_newline_flag_matrix() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-newline-flag-matrix", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_like('a\nb', '^b$', 'n'), regexp_like('a\nb', '^b$', 'm'), regexp_like('a\nb', '^b$', 'p'), regexp_like('a\nb', '^b$', 'w'), regexp_like('a\nb', '^b$', 's'), regexp_like('a\nb', 'a.b', 'n'), regexp_like('a\nb', 'a.b', 'm'), regexp_like('a\nb', 'a.b', 'p'), regexp_like('a\nb', 'a.b', 'w'), regexp_like('a\nb', 'a.b', 's')",
    );

    // Assert
    assert_eq!(
        result.expect("PostgreSQL newline-sensitive anchor and dot behavior").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_apply_the_last_conflicting_regex_mode_flag() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-flag-precedence", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_like('A', 'a', 'ic'), regexp_like('A', 'a', 'ci'), regexp_like('a\nb', 'a.b', 'ns'), regexp_like('a\nb', 'a.b', 'sn')",
    );

    // Assert
    assert_eq!(
        result.expect("last conflicting regular-expression mode flag").rows(),
        &[vec![
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
        ]]
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
fn should_count_the_longest_alternative_before_advancing() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-leftmost-longest", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('aaa', 'a|aa')");

    // Assert
    assert_eq!(
        result.expect("leftmost-longest regexp_count matches").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_keep_leftmost_longest_count_selection_for_lazy_quantifiers() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-lazy-selection", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('aaa', 'a+?')");

    // Assert
    assert_eq!(
        result.expect("leftmost-longest lazy-quantifier count").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_case_fold_unicode_letters_in_regexp_like() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-like-unicode-case", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('é', 'É', 'i')");

    // Assert
    assert_eq!(
        result.expect("Unicode case-insensitive regexp_like").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_count_unicode_case_insensitive_regex_matches() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-unicode-case", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('Éé', 'é', 1, 'i')");

    // Assert
    assert_eq!(
        result
            .expect("Unicode case-insensitive regexp_count")
            .rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_match_unicode_letters_with_the_regex_word_class() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-like-unicode-word-class", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_like('猫', '\\w')");

    // Assert
    assert_eq!(
        result.expect("Unicode regular-expression word class").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_count_nonoverlapping_zero_width_regex_matches() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-zero-width", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('aaa', 'a*')");

    // Assert
    assert_eq!(
        result.expect("non-overlapping zero-width regexp_count matches").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_count_regex_matches_from_a_one_based_unicode_character_start() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-unicode-start", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('éaé', 'é', 2)");

    // Assert
    assert_eq!(
        result.expect("regexp_count Unicode character start").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_count_an_empty_match_at_the_end_position() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-empty-at-end", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('abc', '', 4)");

    // Assert
    assert_eq!(
        result.expect("empty match at end position").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_count_empty_input_position_boundaries() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-empty-input-boundaries", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_count('', ''), regexp_count('', 'a'), regexp_count('', '', 1), regexp_count('', '', 2), regexp_count('', '', 2147483647)",
    );

    // Assert
    assert_eq!(
        result.expect("empty regexp_count input boundaries").rows(),
        &[vec![
            Value::Int64(1),
            Value::Int64(0),
            Value::Int64(1),
            Value::Int64(0),
            Value::Int64(0),
        ]]
    );
}

#[test]
fn should_return_zero_for_maximum_regexp_count_start_on_nonempty_text() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-maximum-start", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_count('abc', '', 2147483647), regexp_count('abc', 'a', 2147483647)",
    );

    // Assert
    assert_eq!(
        result.expect("maximum regexp_count start on nonempty text").rows(),
        &[vec![Value::Int64(0), Value::Int64(0)]]
    );
}

#[test]
fn should_return_zero_when_regexp_count_starts_beyond_the_end_position() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-beyond-end", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('abc', '', 5)");

    // Assert
    assert_eq!(
        result.expect("no matches beyond end position").rows(),
        &[vec![Value::Int64(0)]]
    );
}

#[test]
fn should_propagate_null_inputs_through_selected_regex_functions() {
    // Arrange
    let fixture = sql_fixture("issue-799-selected-regex-null-inputs", &[]);

    // Act
    let result = fixture.execute(
        "SELECT regexp_like(NULL::TEXT, 'x'), regexp_like('x', NULL::TEXT), regexp_like('x', 'x', NULL::TEXT), regexp_count(NULL::TEXT, 'x'), regexp_count('x', NULL::TEXT), regexp_count('abc', 'a', NULL::INT4), regexp_count('abc', 'a', 1, NULL::TEXT), regexp_count('abc', 'a', NULL::INT4, NULL::TEXT), regexp_like(string => 'x', pattern => 'x', flags => NULL::TEXT), regexp_count(string => 'abc', pattern => 'a', start => NULL::INT4), regexp_count(string => 'abc', pattern => 'a', start => 1, flags => NULL::TEXT)",
    );

    // Assert
    assert_eq!(
        result.expect("NULL regular-expression inputs").rows(),
        &[vec![
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ]]
    );
}

#[test]
fn should_report_2201b_for_invalid_regexp_like_patterns() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-invalid-pattern", &[]);

    // Act
    let error = fixture
        .execute("SELECT regexp_like('x', '[')")
        .expect_err("invalid pattern should fail");

    // Assert
    assert!(matches!(
        error,
        CassieError::Diagnostic(ref diagnostic) if diagnostic.sql_state == "2201B"
    ));
}

#[test]
fn should_report_2201b_for_invalid_regexp_like_flags() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-invalid-flag", &[]);

    // Act
    let error = fixture
        .execute("SELECT regexp_like('x', 'x', 'z')")
        .expect_err("invalid flags should fail");

    // Assert
    assert!(matches!(
        error,
        CassieError::Diagnostic(ref diagnostic) if diagnostic.sql_state == "2201B"
    ));
}

#[test]
fn should_reject_postgres_are_backreferences_outside_the_selected_regex_profile() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-like-are-boundary", &[]);

    // Act
    let error = fixture
        .execute("SELECT regexp_like('abab', '^(ab)\\1$')")
        .expect_err("backreferences are outside the selected regex profile");

    // Assert
    assert!(matches!(
        error,
        CassieError::Diagnostic(ref diagnostic) if diagnostic.sql_state == "2201B"
    ));
}

#[test]
fn should_reject_nonpositive_regexp_count_start_positions() {
    // Arrange
    let fixture = sql_fixture("issue-799-regexp-count-invalid-start", &[]);

    // Act
    let result = fixture.execute("SELECT regexp_count('x', 'x', 0)");

    // Assert
    assert!(matches!(
        result.expect_err("zero start position should fail"),
        CassieError::InvalidParameterValue(_)
    ));
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
fn should_evaluate_pg_catalog_qualified_regex_function_aliases() {
    // Arrange
    let fixture = sql_fixture("issue-799-qualified-regex-functions", &[]);

    // Act
    let result = fixture.execute(
        "SELECT pg_catalog.regexp_like('Thomas', 'thomas', 'i'), pg_catalog.regexp_count('ABCABC', 'A.', 1, 'i'), pg_catalog.regexp_like(string => 'Hello', pattern => 'hello', flags => 'i'), pg_catalog.regexp_count(string => 'ABCABC', pattern => 'A.', start => 1, flags => 'i')",
    );

    // Assert
    assert_eq!(
        result.expect("qualified regular-expression function aliases").rows(),
        &[vec![
            Value::Bool(true),
            Value::Int64(2),
            Value::Bool(true),
            Value::Int64(2),
        ]]
    );
}

#[test]
fn should_reject_invalid_selected_regex_named_arguments() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-invalid-named-arguments", &[]);
    let cases = [
        (
            "SELECT regexp_like(string => 'x', pattern => 'x', pattern => 'y')",
            "duplicate named argument 'pattern'",
        ),
        (
            "SELECT regexp_like('x', pattern => 'x')",
            "cannot mix positional and named function arguments",
        ),
        (
            "SELECT regexp_like(string => 'x')",
            "requires named arguments 'string' and 'pattern'",
        ),
        (
            "SELECT regexp_count(string => 'x', pattern => 'x', mode => 'i')",
            "unknown named argument 'mode'",
        ),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, expected_message), result) in cases.iter().zip(results) {
        let error = result.expect_err("invalid named arguments should fail");
        match error {
            CassieError::InvalidQuery(message) => assert!(
                message.contains(expected_message),
                "{sql}: expected {expected_message:?}, got {message:?}"
            ),
            other => panic!("{sql}: expected a parser error, got {other:?}"),
        }
    }
}

#[test]
fn should_reject_unsupported_positional_regex_function_arities() {
    // Arrange
    let fixture = sql_fixture("issue-799-regex-invalid-positional-arities", &[]);
    let cases = [
        ("SELECT regexp_like('x')", "regexp_like"),
        ("SELECT regexp_like('x', 'x', 'i', 'extra')", "regexp_like"),
        ("SELECT regexp_count('x')", "regexp_count"),
        (
            "SELECT regexp_count('x', 'x', 1, 'i', 'extra')",
            "regexp_count",
        ),
    ];

    // Act
    let results = cases
        .iter()
        .map(|(sql, _)| fixture.execute(sql))
        .collect::<Vec<_>>();

    // Assert
    for ((sql, function), result) in cases.iter().zip(results) {
        let error = result.expect_err("unsupported arity should fail");
        match error {
            CassieError::Planner(message) => assert!(
                message.starts_with(&format!("function '{function}' expects "))
                    && message.contains(", got "),
                "{sql}: expected an invalid arity error, got {message:?}"
            ),
            other => panic!("{sql}: expected an invalid arity error, got {other:?}"),
        }
    }
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
