#[test]
fn should_match_closed_numeric_are_backreferences() {
    // Arrange
    let fixture = sql_fixture("issue-799-backreferences", &[]);
    let cases = [
        ("abab", r"^(ab)\1$", "", true),
        ("abac", r"^(ab)\1$", "", false),
        ("éé", r"^(é)\1$", "", true),
        ("Éé", r"^(É)\1$", "i", true),
        ("b", r"^(a)?b\1$", "", false),
        ("b", r"^(a*)b\1$", "", true),
        ("aba", r"^(a)(?=(b))b\1$", "", true),
        ("abab", r"^(?=(a))(ab)\1$", "", true),
        ("a\na\n", r"^(a.)\1$", "", true),
        ("a\na\n", r"^(a.)\1$", "n", false),
    ];

    // Act
    let results = cases.map(|(input, pattern, flags, _)| fixture.cassie.execute_sql(
        &fixture.session, "SELECT regexp_like($1,$2,$3)",
        vec![Value::String(input.into()), Value::String(pattern.into()), Value::String(flags.into())],
    ));

    // Assert
    for ((input, pattern, flags, expected), result) in cases.into_iter().zip(results) {
        assert_eq!(result.expect("selected backreference control").rows(), &[vec![Value::Bool(expected)]],
            "input={input:?}, pattern={pattern:?}, flags={flags:?}");
    }
}

#[test]
fn should_evaluate_are_lookarounds_in_full_input_context() {
    // Arrange
    let fixture = sql_fixture("issue-799-lookarounds", &[]);
    let cases = [
        ("ab", "a(?=b)", true), ("ac", "a(?=b)", false),
        ("ac", "a(?!b)", true), ("ab", "a(?!b)", false),
        ("ab", "(?<=a)b", true), ("cb", "(?<=a)b", false),
        ("cb", "(?<!a)b", true), ("ab", "(?<!a)b", false),
        ("abbc", "(?<=ab*)c", true), ("bbbc", "(?<!ab*)c", true),
        ("ééδ", "(?<=é+)δ", true),
        ("aaab", "(?<=(?=a{3})a*)b", true),
        ("abc", "(?<=^ab)c", true), ("xabc", "(?<=^ab)c", false),
        ("cat!", r"(?<=\mcat)\M!", true),
        ("a\nb", r"(?<=[^x]+)b", true),
    ];

    // Act
    let results = cases.map(|(input, pattern, _)| fixture.cassie.execute_sql(
        &fixture.session, "SELECT regexp_like($1,$2)",
        vec![Value::String(input.into()), Value::String(pattern.into())],
    ));

    // Assert
    for ((input, pattern, expected), result) in cases.into_iter().zip(results) {
        assert_eq!(result.expect("pinned lookaround control").rows(), &[vec![Value::Bool(expected)]],
            "input={input:?}, pattern={pattern:?}");
    }
}

#[test]
fn should_preserve_greedy_are_capture_priorities() {
    // Arrange
    let fixture = sql_fixture("issue-799-constraint-captures", &[]);
    let cases = [
        ("ababa", ".*(..).*(?=)", "ba|"),
        ("aaa", "(a|aa)(a?)(?=)", "aa|a"),
        ("abab", "((ab)+)(?=)", "abab|ab"),
        ("aaa", "(a?)*(?=)", "a|"),
        ("aaa", "(a|aa)*(?=)", "a|"),
        ("ab", "((a)|b)+(?=)", "b|"),
        ("ab", "((a)|ab)(?=)", "ab|"),
        ("abcabc", r"(abc)\1", "abc|"),
        ("a", "(a?){2}(?=)", "|"),
        ("", "(a?){2}(?=)", "|"),
        ("aaa", "((a|aa)*)*(?=)", "aaa|a"),
        ("abab", "((a|ab)+)*(?=)", "abab|ab"),
        ("aaa", "((a?)*)+(?=)", "|"),
        ("aa", r"((a?)*)\1", "a|a"),
        ("aaa", r"((a?){2})\1", "a|a"),
        ("aaab", "(a?){2,4}b(?=)", "|"),
        ("ab", "((a)?b?){2}(?=)", "|"),
        ("ééδ", "(é|éé)*(?=δ)", "éé|δ"),
    ];

    // Act
    let results = cases.map(|(input, pattern, _)| fixture.cassie.execute_sql(
        &fixture.session, "SELECT regexp_replace($1,$2,$3)",
        vec![Value::String(input.into()), Value::String(pattern.into()), Value::String(r"\1|\2".into())],
    ));

    // Assert
    for ((input, pattern, expected), result) in cases.into_iter().zip(results) {
        assert_eq!(result.expect("pinned greedy capture control").rows(), &[vec![Value::String(expected.into())]],
            "input={input:?}, pattern={pattern:?}");
    }
}

#[test]
fn should_share_are_constraints_with_count_replace_and_quantifiers() {
    // Arrange
    let fixture = sql_fixture("issue-799-shared-constraints", &[]);

    // Act
    let result = fixture.execute(r"SELECT regexp_count('abab ab', '(ab)\1'), regexp_replace('abab abab', '(ab)\1', '<\1>', 'g'), 'abab' ~ ANY(ARRAY['x','(ab)\1']), regexp_count('ééδééδ','(?<=é+)δ')");

    // Assert
    assert_eq!(result.expect("shared constraint runtime").rows(), &[vec![
        Value::Int64(1), Value::String("<ab> <ab>".into()), Value::Bool(true), Value::Int64(2),
    ]]);
}

#[test]
fn should_reject_unclosed_or_constraint_local_are_backreferences() {
    // Arrange
    let fixture = sql_fixture("issue-799-invalid-backreferences", &[]);
    let patterns = [r"\1(a)", r"(a\1)", r"(?=(a)\1)", r"(?<=(a)\1)b", r"(a)\2"];

    // Act
    let results = patterns.map(|pattern| fixture.cassie.execute_sql(
        &fixture.session, "SELECT regexp_like('aa',$1)", vec![Value::String(pattern.into())],
    ));

    // Assert
    for (pattern, result) in patterns.into_iter().zip(results) {
        assert!(matches!(result, Err(CassieError::Diagnostic(ref diagnostic)) if diagnostic.sql_state == "2201B"), "pattern={pattern:?}");
    }
}

#[test]
fn should_reject_unselected_ast_erased_constraint_notations() {
    // Arrange
    let fixture = sql_fixture("issue-799-constraint-notation-boundary", &[]);
    let patterns = [r"(a)\k<1>", r"\h(?=)", r"\N(?=)", r"(?=)[\m]", r"(?=)[[[:<:]]]", r"(?W)\w(?=)"];

    // Act
    let results = patterns.map(|pattern| fixture.cassie.execute_sql(
        &fixture.session, "SELECT regexp_like('aa',$1)", vec![Value::String(pattern.into())],
    ));

    // Assert
    for (pattern, result) in patterns.into_iter().zip(results) {
        assert!(matches!(result, Err(CassieError::Diagnostic(ref diagnostic)) if diagnostic.sql_state == "2201B"), "pattern={pattern:?}");
    }
}

#[test]
fn should_preserve_ignored_and_escaped_constraint_notation_contexts() {
    // Arrange
    let fixture = sql_fixture("issue-799-constraint-source-context", &[]);
    let cases = [
        ("aa", "(?x)# \\h \\k<1> [ (?W)\n(a)\\1", ""),
        ("aa", r"(?#\h \k<1> [)(a)\1", ""),
        (r"\h", r"\\h(?=)", ""),
        ("m", r"[\\m](?=)", ""),
        ("a#", r"(?x:(a))(?-x:#)(?=)", ""),
        ("]", r"[]](?=)", ""),
        ("a", r"[^^](?=)", ""),
        ("a", r"(?=(a))(?=(b|a))(a)\1?", ""),
    ];

    // Act
    let results = cases.map(|(input, pattern, flags)| fixture.cassie.execute_sql(
        &fixture.session, "SELECT regexp_like($1,$2,$3)",
        vec![Value::String(input.into()), Value::String(pattern.into()), Value::String(flags.into())],
    ));

    // Assert
    for ((input, pattern, _), result) in cases.into_iter().zip(results) {
        assert_eq!(result.expect("source notation context").rows(), &[vec![Value::Bool(true)]], "input={input:?}, pattern={pattern:?}");
    }
}

#[test]
fn should_preserve_pg_empty_progress_backreference_results() {
    // Arrange
    let fixture = sql_fixture("issue-799-empty-progress-references", &[]);
    let cases = [
        ("b", r"^(?:(a?)|b\1)*$", false),
        ("b", r"^(?:(a?)|b\1)+$", false),
        ("a", r"^(a?)*\1$", false),
        ("aaa", r"^(a?)*\1$", true),
        ("", r"^()*\1$", true),
        ("a", r"^((a?)*)\1$", false),
    ];

    // Act
    let results = cases.map(|(input, pattern, _)| fixture.cassie.execute_sql(
        &fixture.session, "SELECT regexp_like($1,$2)", vec![Value::String(input.into()), Value::String(pattern.into())],
    ));

    // Assert
    for ((input, pattern, expected), result) in cases.into_iter().zip(results) {
        assert_eq!(result.expect("pinned empty-progress result").rows(), &[vec![Value::Bool(expected)]], "input={input:?}, pattern={pattern:?}");
    }
}
