#[test]
fn should_match_selected_are_word_assertions() {
    // Arrange
    let fixture = sql_fixture("issue-799-are-assertions", &[]);
    let cases = [
        ("cat!", r"\mcat\M", "", true),
        ("bobcat!", r"\mcat\M", "", false),
        ("cat!", r"cat\y", "", true),
        ("catx", r"cat\Y", "", true),
        ("\u{0008}", r"\b", "", true),
        ("cat", r"\bcat\b", "", false),
        ("\\", r"\B", "", true),
        ("x", r"x\Z", "", true),
        ("x\n", r"x\Z", "", false),
        (r"\m", r"\m", "q", true),
        ("m", r"\m", "b", true),
        ("m", r"\m", "e", true),
        ("b", r"\b", "b", true),
        ("b", r"\b", "e", true),
        ("cat!", r"[[:<:]]cat[[:>:]]", "", true),
        ("cat!", "# [\n\\mcat\\M", "x", true),
        (r"\m", r"\\m", "", true),
    ];

    // Act
    let results = cases.map(|(input, pattern, flags, _)| {
        fixture.cassie.execute_sql(
            &fixture.session,
            "SELECT regexp_like($1,$2,$3)",
            vec![
                Value::String(input.into()),
                Value::String(pattern.into()),
                Value::String(flags.into()),
            ],
        )
    });

    // Assert
    for ((input, pattern, flags, expected), result) in cases.into_iter().zip(results) {
        assert_eq!(
            result.expect("pinned assertion control").rows(),
            &[vec![Value::Bool(expected)]],
            "input={input:?}, pattern={pattern:?}, flags={flags:?}"
        );
    }
}

#[test]
fn should_use_are_assertions_in_count_replacement() {
    // Arrange
    let fixture = sql_fixture("issue-799-are-scalar-assertions", &[]);

    // Act
    let result = fixture.execute(r"SELECT regexp_count('one two','\m'), regexp_replace('one two','\m','<','g'), '-abc-' SIMILAR TO '%\mabc\M%'");

    // Assert
    assert_eq!(
        result.expect("shared assertion preparation").rows(),
        &[vec![
            Value::Int64(2),
            Value::String("<one <two".into()),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_reject_are_constraints_inside_bracket_classes() {
    // Arrange
    let fixture = sql_fixture("issue-799-are-invalid-class-constraints", &[]);
    let patterns = [r"[\m]", r"[\Y]", r"[\Z]", r"[[[:<:]]]", r"[^\m]"];

    // Act
    let results = patterns.map(|pattern| {
        fixture.cassie.execute_sql(
            &fixture.session,
            "SELECT regexp_like('x',$1)",
            vec![Value::String(pattern.into())],
        )
    });

    // Assert
    for result in results {
        assert!(
            matches!(result,Err(CassieError::Diagnostic(ref error)) if error.sql_state == "2201B")
        );
    }
}

#[test]
fn should_preserve_are_comment_bracket_director_contexts() {
    // Arrange
    let fixture = sql_fixture("issue-799-are-context-controls", &[]);
    let cases = [
        ("cat!", "# \\m [[:<:]] [\n\\mcat\\M", "x", true),
        ("cat!", "(?x)# [[:<:]] [\n\\mcat\\M", "", true),
        ("#cat!", "\\#\\mcat\\M", "x", true),
        ("]cat!", "[\\]]\\mcat\\M", "", true),
        ("cat!", "***:\\mcat\\M", "b", true),
        ("cat!", "[[:<:]]cat[[:>:]]", "e", true),
        ("cat!", "[[:<:]]cat[[:>:]]", "b", true),
    ];

    // Act
    let observations = cases.map(|(input, pattern, flags, _)| {
        fixture.cassie.execute_sql(
            &fixture.session,
            "SELECT regexp_like($1,$2,$3)",
            vec![
                Value::String(input.into()),
                Value::String(pattern.into()),
                Value::String(flags.into()),
            ],
        )
    });

    // Assert
    for ((input, pattern, flags, expected), observation) in cases.into_iter().zip(observations) {
        assert_eq!(
            observation.expect("pinned lexical context").rows(),
            &[vec![Value::Bool(expected)]],
            "input={input:?}, pattern={pattern:?}, flags={flags:?}"
        );
    }
}
