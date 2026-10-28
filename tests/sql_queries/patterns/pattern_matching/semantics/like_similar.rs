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
fn should_match_newlines_with_similar_to_wildcards() {
    // Arrange
    let fixture = sql_fixture("issue-799-similar-to-newlines", &[]);

    // Act
    let result = fixture.execute("SELECT 'a\nb' SIMILAR TO 'a%b', 'a\nb' SIMILAR TO 'a_b'");

    // Assert
    assert_eq!(
        result.expect("SIMILAR TO wildcards include newline characters").rows(),
        &[vec![Value::Bool(true), Value::Bool(true)]]
    );
}

#[test]
fn should_preserve_similar_escape_metacharacter_semantics() {
    // Arrange
    let fixture = sql_fixture("issue-799-similar-to-pattern-metacharacters", &[]);

    // Act
    let result = fixture.execute(
        r"SELECT 'a%b' SIMILAR TO 'a\%b', 'aaa' SIMILAR TO 'a+', 'a.b' SIMILAR TO 'a.b', 'a%b' SIMILAR TO 'a!%b' ESCAPE ''",
    );

    // Assert
    assert_eq!(
        result.expect("SIMILAR TO escape and metacharacter semantics").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_propagate_nulls_for_similar_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-799-similar-to-null-pattern", &[]);

    // Act
    let result = fixture.execute(
        "SELECT NULL SIMILAR TO 'x', 'x' SIMILAR TO NULL, 'x' SIMILAR TO 'x' ESCAPE NULL",
    );

    // Assert
    assert_eq!(
        result.expect("SIMILAR TO NULL pattern and escape").rows(),
        &[vec![Value::Null, Value::Null, Value::Null]]
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

