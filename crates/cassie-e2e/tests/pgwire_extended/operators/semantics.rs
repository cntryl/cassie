type QuantifiedCase = (
    &'static str,
    &'static str,
    &'static str,
    String,
    Option<&'static str>,
);

fn assert_quantified_results(cases: &[QuantifiedCase], results: &[Vec<(u8, Vec<u8>)>]) {
    for ((scenario, quantifier, operator, sql, expected), result) in cases.iter().zip(results) {
        assert_eq!(
            wire::error_code(result),
            None,
            "unexpected error for {scenario} {quantifier} {operator}: {sql}; ErrorResponse: {:?}",
            result
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
                .collect::<Vec<_>>()
        );
        let description = result
            .iter()
            .find(|frame| frame.0 == b'T')
            .unwrap_or_else(|| panic!("missing Boolean RowDescription for {sql}"));
        assert_eq!(
            wire::parse_row_description(&description.1)[0].type_oid,
            16,
            "comparison result type for {sql}"
        );
        assert_eq!(
            wire::data_rows(result),
            vec![vec![expected.map(str::to_owned)]],
            "PostgreSQL 18.6 truth value for {sql}"
        );
    }
}

#[test]
fn should_match_quantified_comparison_operator_truth_tables() {
    // Arrange
    let operators = ["=", "<>", "<", "<=", ">", ">="];
    let scenarios = [
        (
            "non_null",
            "scenario = 'non_null'",
            [
                Some("f"),
                Some("t"),
                Some("t"),
                Some("t"),
                Some("t"),
                Some("t"),
            ],
            [
                Some("f"),
                Some("t"),
                Some("f"),
                Some("f"),
                Some("f"),
                Some("f"),
            ],
        ),
        (
            "with_null",
            "scenario = 'with_null'",
            [None, Some("t"), Some("t"), Some("t"), Some("t"), Some("t")],
            [Some("f"), None, Some("f"), Some("f"), Some("f"), Some("f")],
        ),
        ("empty", "false", [Some("f"); 6], [Some("t"); 6]),
    ];
    let mut cases: Vec<QuantifiedCase> = Vec::new();
    let mut steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_762_quantifier_values (scenario TEXT, value INT4)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_762_quantifier_values VALUES \
             ('non_null', 1), ('non_null', 3), \
             ('with_null', 1), ('with_null', 3), ('with_null', NULL)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT value FROM issue_762_quantifier_values \
             WHERE scenario = 'non_null' ORDER BY value",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 2 < ANY (SELECT value FROM issue_762_quantifier_values \
             WHERE scenario = 'non_null') AS scalar_any_control",
        )]),
    ];
    for (scenario, predicate, any_results, all_results) in scenarios {
        for (quantifier, results) in [("ANY", any_results), ("ALL", all_results)] {
            for (index, operator) in operators.iter().enumerate() {
                let sql = format!(
                    "SELECT 2 {operator} {quantifier} \
                     (SELECT value FROM issue_762_quantifier_values WHERE {predicate}) \
                     AS comparison_result"
                );
                cases.push((scenario, quantifier, *operator, sql, results[index]));
                steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
                    cases.last().expect("just-added comparison case").3.as_str(),
                )]));
            }
        }
    }

    // Act
    let batches = run_script("issue-762-any-all-operator-truth-tables", steps);

    // Assert
    assert_eq!(batches.len(), cases.len() + 4);
    assert_subquery_result(
        &batches[2],
        &[23],
        &[vec![Some("1".to_owned())], vec![Some("3".to_owned())]],
        "same-source quantifier table control",
    );
    assert_subquery_result(
        &batches[3],
        &[16],
        &[vec![Some("t".to_owned())]],
        "same-source scalar ANY control",
    );
    assert_quantified_results(&cases, &batches[4..]);
}
