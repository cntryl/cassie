#[test]
fn should_reject_user_defined_collation_index_grouping_over_pgwire() {
    // Arrange
    let batches = run_script(
        "issue-796-user-collation-out-of-scope",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE COLLATION issue_796_ci_index (provider = icu, locale = 'und-u-ks-level2', deterministic = false)",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("0A000"));
}

#[test]
fn should_return_one_dimensional_subtotals_over_pgwire() {
    // Arrange
    let aggregates = "sum(amount) AS total, count(DISTINCT amount) FILTER (WHERE amount > 0) AS c, array_agg(id ORDER BY id DESC) AS ids";
    let queries = [
        format!("SELECT region, grouping(region) AS g, {aggregates} FROM report GROUP BY ROLLUP(region) ORDER BY 2,1 NULLS LAST"),
        format!("SELECT region, grouping(region) AS g, {aggregates} FROM report GROUP BY GROUPING SETS ((region), ()) ORDER BY 2,1 NULLS LAST"),
        format!("SELECT region,g,total,c,ids FROM (SELECT region,0::int AS g,{aggregates} FROM report GROUP BY region UNION ALL SELECT NULL::text AS region,1::int AS g,{aggregates} FROM report) u ORDER BY 2,1 NULLS LAST"),
    ];
    let populated = [
        [Some("A"), Some("0"), Some("20"), Some("1"), Some("{2,1}")],
        [None, Some("0"), Some("7"), Some("1"), Some("{3}")],
        [None, Some("1"), Some("27"), Some("2"), Some("{3,2,1}")],
    ];
    let same_sum = [
        [None, Some("0"), Some("7"), Some("1"), Some("{3}")],
        [None, Some("1"), Some("7"), Some("1"), Some("{3}")],
    ];
    let empty = [[None, Some("1"), None, Some("0"), None]];
    let cases = [
        (
            "INSERT INTO report VALUES (1,'A',10),(2,'A',10),(3,NULL,7)",
            populated.as_slice(),
        ),
        (
            "INSERT INTO report VALUES (3,NULL,7),(2,'A',10),(1,'A',10)",
            populated.as_slice(),
        ),
        ("INSERT INTO report VALUES (3,NULL,7)", same_sum.as_slice()),
        ("DELETE FROM report", empty.as_slice()),
    ];
    let simple = |sql: &str| Step::ThroughReady(vec![wire::simple_query_frame(sql)]);
    let mut steps = vec![simple("CREATE TABLE report(id int,region text,amount int)")];
    let mut results = Vec::new();
    for (insert, expected) in cases {
        steps.push(simple("DELETE FROM report"));
        steps.push(simple(insert));
        for query in &queries {
            steps.push(Step::ParseFlush(wire::parse_frame_with_types(
                "",
                query,
                &[],
            )));
            results.push((steps.len(), expected));
            steps.push(Step::ThroughReady(vec![
                wire::bind_frame_with_formats("", "", &[], &[], &[]),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
        }
    }

    // Act
    let batches = run_script("one-dimensional-report", steps);

    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    for (index, expected) in results {
        let batch = &batches[index];
        let columns = batch
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("row description");
        assert_eq!(
            wire::parse_row_description(&columns.1)
                .iter()
                .map(|field| field.type_oid)
                .collect::<Vec<_>>(),
            [25, 23, 20, 20, 1007]
        );
        let expected: Vec<Vec<_>> = expected
            .iter()
            .map(|row| row.iter().map(|value| value.map(str::to_owned)).collect())
            .collect();
        assert_eq!(wire::data_rows(batch), expected);
    }
}

#[test]
fn should_validate_grouping_arguments_in_their_query_level() {
    // Arrange
    let cases = [
        (
            "SELECT grouping(amount) FROM report GROUP BY ROLLUP(region)",
            "42803",
        ),
        ("SELECT grouping(region) FROM report", "42803"),
        (
            "SELECT region FROM report WHERE grouping(region)=0 GROUP BY region",
            "42803",
        ),
        ("SELECT grouping() FROM report GROUP BY region", "42601"),
        (
            "SELECT grouping(region) OVER () FROM report GROUP BY ROLLUP(region)",
            "42601",
        ),
        (
            "SELECT amount+1 AS z,count(*) FROM report GROUP BY ROLLUP(z)",
            "0A000",
        ),
        (
            "SELECT count(*) FROM report GROUP BY CUBE(region,amount)",
            "0A000",
        ),
        (
            "SELECT count(*) FROM report GROUP BY ROLLUP(region,amount)",
            "0A000",
        ),
        (
            "SELECT count(*) FROM report GROUP BY GROUPING SETS ((region),(amount),())",
            "0A000",
        ),
    ];
    let mut steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "CREATE TABLE report(region text,amount int)",
    )])];
    for (sql, _) in cases {
        steps.push(Step::ParseFlush(wire::parse_frame_with_types("", sql, &[])));
        steps.push(Step::ThroughReady(vec![wire::sync_frame()]));
    }

    // Act
    let batches = run_script("invalid-one-dimensional-grouping", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let (pairs, remainder) = batches[1..].as_chunks::<2>();
    assert_eq!(remainder, []);
    assert_eq!(pairs.len(), cases.len());
    for ((sql, code), pair) in cases.iter().zip(pairs) {
        assert_eq!(wire::error_code(&pair[0]).as_deref(), Some(*code), "{sql}");
    }
}

#[test]
fn should_share_volatile_aggregate_inputs_between_grouping_levels() {
    // Arrange
    let queries = [
        "CREATE TABLE report(region text,amount int)",
        "CREATE SEQUENCE subtotal_values START WITH 1",
        "INSERT INTO report VALUES ('A',1),('A',2),(NULL,3),('B',0)",
        "SELECT grouping(region) AS g,sum(nextval('subtotal_values')) FILTER (WHERE amount>0) AS total FROM report GROUP BY ROLLUP(region) HAVING grouping(region)=1",
        "SELECT currval('subtotal_values')",
        "SELECT grouping(region) AS g FROM report GROUP BY GROUPING SETS (region) ORDER BY g",
    ];
    let steps = queries
        .iter()
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();

    // Act
    let batches = run_script("shared-subtotal-input", steps);

    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::data_rows(&batches[3]),
        [[Some("1".to_owned()), Some("6".to_owned())]]
    );
    assert_eq!(wire::data_rows(&batches[4]), [[Some("3".to_owned())]]);
    assert_eq!(
        wire::data_rows(&batches[5]),
        vec![vec![Some("0".to_owned())]; 3]
    );
}

#[test]
fn should_keep_distinct_occurrences_of_volatile_subtotal_inputs() {
    // Arrange
    let queries = [
        "CREATE TABLE report(region text)",
        "CREATE SEQUENCE repeated_inputs START WITH 1",
        "INSERT INTO report VALUES (NULL),(NULL)",
        "SELECT grouping(region) AS g,sum(nextval('repeated_inputs')) AS s1,sum(nextval('repeated_inputs')) AS s2 FROM report GROUP BY ROLLUP(region) ORDER BY g",
        "SELECT currval('repeated_inputs')",
    ];
    let steps = queries
        .iter()
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();

    // Act
    let batches = run_script("volatile-subtotal-occurrences", steps);

    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::data_rows(&batches[3]),
        [
            [
                Some("0".to_owned()),
                Some("4".to_owned()),
                Some("6".to_owned())
            ],
            [
                Some("1".to_owned()),
                Some("4".to_owned()),
                Some("6".to_owned())
            ],
        ]
    );
    assert_eq!(wire::data_rows(&batches[4]), [[Some("4".to_owned())]]);
}

#[test]
fn should_rank_subtotal_rows_by_the_grouping_indicator() {
    // Arrange
    let queries = [
        "CREATE TABLE report(region text)",
        "INSERT INTO report VALUES (NULL),(NULL)",
        "SELECT region, rank() OVER (ORDER BY grouping(region)) AS n FROM report GROUP BY ROLLUP(region) ORDER BY n",
    ];
    let steps = queries
        .iter()
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();

    // Act
    let batches = run_script("window-over-subtotal-level", steps);

    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::data_rows(&batches[2]),
        [[None, Some("1".to_owned())], [None, Some("2".to_owned())]]
    );
}

#[test]
fn should_select_one_report_row_per_grouping_level() {
    // Arrange
    let sql = [
        "CREATE TABLE report(region text)",
        "INSERT INTO report VALUES ('A'),(NULL)",
        "SELECT DISTINCT ON (grouping(region)) grouping(region) AS g,region FROM report GROUP BY ROLLUP(region) ORDER BY grouping(region),region NULLS LAST",
    ];
    let steps = sql
        .iter()
        .map(|statement| Step::ThroughReady(vec![wire::simple_query_frame(statement)]))
        .collect();

    // Act
    let batches = run_script("distinct-subtotal-level", steps);

    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::data_rows(&batches[2]),
        [
            [Some("0".to_owned()), Some("A".to_owned())],
            [Some("1".to_owned()), None],
        ]
    );
}

#[test]
fn should_preserve_empty_subtotals_in_exists_subqueries() {
    // Arrange
    let queries = [
        "CREATE TABLE report(region text)",
        "SELECT EXISTS (SELECT 1 FROM report GROUP BY ROLLUP(region) LIMIT 1)",
        "SELECT EXISTS (SELECT 1 FROM report GROUP BY GROUPING SETS ((region), ()))",
        "SELECT EXISTS (SELECT 1 FROM report GROUP BY region)",
        "SELECT EXISTS (SELECT 1 FROM report GROUP BY ROLLUP(region) LIMIT 0)",
    ];
    let steps = queries
        .iter()
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();

    // Act
    let batches = run_script("exists-over-empty-subtotal", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    for (batch, expected) in batches[1..].iter().zip(["t", "t", "f", "f"]) {
        assert_eq!(wire::error_code(batch), None);
        assert_eq!(wire::data_rows(batch), [[Some(expected.to_owned())]]);
    }
}

#[test]
fn should_evaluate_correlated_subtotal_inputs_per_aggregate_call() {
    // Arrange
    let input = "(SELECT nextval('correlated_inputs') FROM (VALUES(1)) x(i) WHERE report.region IS NOT NULL)";
    let subtotal = format!(
        "SELECT grouping(report.region),sum({input}),sum({input}),sum({input}) FILTER (WHERE report.region IS NULL) FROM report GROUP BY ROLLUP(report.region) ORDER BY 1"
    );
    let queries = [
        "CREATE TABLE report(region text)",
        "CREATE SEQUENCE correlated_inputs START WITH 1",
        "INSERT INTO report VALUES ('A'),('A')",
        &subtotal,
        "SELECT currval('correlated_inputs')",
    ];
    let steps = queries
        .iter()
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();

    // Act
    let batches = run_script("correlated-subtotal-inputs", steps);

    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::data_rows(&batches[3]),
        ["0", "1"].map(|flag| [
            Some(flag.to_owned()),
            Some("4".to_owned()),
            Some("6".to_owned()),
            None,
        ])
    );
    assert_eq!(wire::data_rows(&batches[4]), [[Some("4".to_owned())]]);
}
