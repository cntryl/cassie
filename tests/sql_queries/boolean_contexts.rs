//! Boolean typing through the supported query and storage paths.

use super::support_sql::{assert_explain_contains, explain_plan_text};
use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::executor::QueryResult;
use cassie::types::Value;

#[test]
fn should_distinguish_boolean_search_predicates_from_numeric_search_scores() {
    // Arrange
    let fixture = sql_fixture(
        "bool-search-function-types",
        &[
            "CREATE TABLE bool_search (id INT, body TEXT)",
            "INSERT INTO bool_search VALUES (1, 'alpha'), (2, 'bravo')",
        ],
    );

    // Act
    let literal = fixture.execute("SELECT search('alpha', 'alpha') AS matched");
    let predicate = fixture.execute(
        "SELECT id, search(body, 'alpha') AS matched FROM bool_search WHERE search(body, 'alpha') ORDER BY id",
    );
    let negated = fixture.execute("SELECT NOT search('alpha', 'bravo') AS matched");
    let score = fixture.execute("SELECT search_score('alpha', 'alpha') AS score");
    let numeric_predicate = fixture.execute("SELECT 1 WHERE search_score('alpha', 'alpha')");

    // Assert
    let literal = literal.expect("search produces a Boolean value");
    assert_eq!(literal.rows, vec![vec![Value::Bool(true)]]);
    assert_eq!(literal.columns[0].type_oid, 16);
    let predicate = predicate.expect("Boolean search predicate remains supported");
    assert_eq!(
        predicate.rows,
        vec![vec![Value::Int64(1), Value::Bool(true)]]
    );
    assert_eq!(predicate.columns[1].type_oid, 16);
    let negated = negated.expect("Boolean search result supports NOT");
    assert_eq!(negated.rows, vec![vec![Value::Bool(true)]]);
    assert_eq!(negated.columns[0].type_oid, 16);
    let score = score.expect("search_score retains numeric semantics");
    assert_eq!(score.columns[0].type_oid, 701);
    assert!(matches!(score.rows[0][0], Value::Float64(value) if value > 0.0));
    assert!(matches!(numeric_predicate, Err(CassieError::Planner(_))));
}

fn assert_static_type_error(result: &Result<QueryResult, CassieError>, sql: &str) {
    assert!(
        matches!(result, Err(CassieError::Planner(_))),
        "a statically typed Boolean-context error must fail in binding: {sql}"
    );
}

fn typed_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE bool_typed (n INT, txt TEXT, flag BOOLEAN)",
            "INSERT INTO bool_typed (n, txt, flag) VALUES (1, 'no', TRUE), (2, 'yes', FALSE)",
            "CREATE TABLE bool_empty (n INT, txt TEXT, flag BOOLEAN)",
        ],
    )
}

#[test]
fn should_contextualize_join_predicates_before_operand_family_validation() {
    // Arrange
    let fixture = typed_fixture("bool-join-early-context");
    fixture
        .execute("CREATE TABLE bool_rhs (other INT)")
        .expect("create right input");
    fixture
        .execute("INSERT INTO bool_rhs VALUES (1)")
        .expect("seed right input");
    let statements = [
        "SELECT bool_typed.n FROM bool_typed JOIN bool_rhs ON bool_typed.flag = 'no'",
        "SELECT bool_typed.n FROM bool_typed JOIN bool_rhs ON CASE WHEN 'no' THEN TRUE ELSE FALSE END",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    assert_eq!(
        results[0]
            .as_ref()
            .expect("Boolean comparison before source validation")
            .rows,
        vec![vec![Value::Int64(2)]]
    );
    assert_eq!(
        results[1]
            .as_ref()
            .expect("searched CASE before source validation")
            .rows,
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_preserve_typed_boolean_comparison_provenance() {
    // Arrange
    let fixture = typed_fixture("bool-comparison-typed-text");
    let statements = [
        "SELECT n FROM bool_typed WHERE flag = CAST('no' AS TEXT)",
        "SELECT n FROM bool_typed WHERE flag = txt",
        "SELECT n FROM bool_typed WHERE flag = CASE WHEN TRUE THEN 'no' ELSE 'yes' END",
        "SELECT n FROM bool_empty WHERE flag IN (CAST('no' AS TEXT))",
        "SELECT CASE flag WHEN CAST('no' AS TEXT) THEN 1 ELSE 2 END FROM bool_empty",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in statements.into_iter().zip(results) {
        assert_static_type_error(&result, sql);
    }
}

#[test]
fn should_reject_typed_non_boolean_where_roots() {
    // Arrange
    let fixture = typed_fixture("bool-where-types");
    let statements = [
        "SELECT 1 WHERE 1",
        "SELECT 1 WHERE 0.5",
        "SELECT 1 WHERE CAST('no' AS TEXT)",
        "SELECT n FROM bool_typed WHERE n",
        "SELECT n FROM bool_typed WHERE txt",
        "SELECT n FROM bool_typed WHERE lower(txt)",
        "SELECT n FROM bool_typed WHERE length(txt)",
        "SELECT n FROM bool_typed WHERE abs(n)",
        "SELECT 1 WHERE CASE WHEN TRUE THEN 'no' ELSE 'yes' END",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in statements.into_iter().zip(results) {
        assert_static_type_error(&result, sql);
    }
}

#[test]
fn should_reject_typed_boolean_operator_operands_despite_a_determined_result() {
    // Arrange
    let fixture = typed_fixture("bool-operator-types");
    let statements = [
        "SELECT 1 AND TRUE",
        "SELECT FALSE AND 1",
        "SELECT NULL AND 1",
        "SELECT TRUE OR 0",
        "SELECT 0 OR FALSE",
        "SELECT NULL OR CAST('no' AS TEXT)",
        "SELECT NOT 1",
        "SELECT NOT CAST('no' AS TEXT)",
        "SELECT flag AND txt FROM bool_typed",
        "SELECT n OR flag FROM bool_typed",
        "SELECT CASE WHEN FALSE THEN (1 AND TRUE) ELSE FALSE END",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in statements.into_iter().zip(results) {
        assert_static_type_error(&result, sql);
    }
}

#[test]
fn should_validate_boolean_contexts_before_suppressed_row_evaluation() {
    // Arrange
    let fixture = typed_fixture("bool-empty-limit-zero");
    let statements = [
        "SELECT n FROM bool_empty WHERE n",
        "SELECT n FROM bool_empty WHERE CAST('no' AS TEXT)",
        "SELECT n FROM bool_typed WHERE n LIMIT 0",
        "SELECT 1 AND TRUE FROM bool_empty",
        "SELECT NOT txt FROM bool_empty",
        "SELECT COUNT(*) FROM bool_empty HAVING COUNT(*)",
        "SELECT COUNT(*) FROM bool_typed HAVING 1 LIMIT 0",
        "SELECT CASE WHEN 1 THEN TRUE ELSE FALSE END FROM bool_empty",
        "SELECT n FROM bool_empty WHERE EXISTS (SELECT n FROM bool_typed WHERE 1)",
        "SELECT n FROM bool_typed WHERE EXISTS (SELECT n FROM bool_empty WHERE 1) LIMIT 0",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in statements.into_iter().zip(results) {
        assert_static_type_error(&result, sql);
    }
}

#[test]
fn should_reject_non_boolean_relation_predicate_roots() {
    // Arrange
    let fixture = typed_fixture("bool-join-having-types");
    let statements = [
        "SELECT bool_typed.n FROM bool_typed JOIN bool_empty ON 1",
        "SELECT bool_typed.n FROM bool_typed LEFT JOIN bool_empty ON bool_typed.n",
        "SELECT bool_typed.n FROM bool_typed RIGHT JOIN bool_empty ON CAST('no' AS TEXT)",
        "SELECT bool_typed.n FROM bool_typed FULL OUTER JOIN bool_empty ON 0.5",
        "SELECT COUNT(*) FROM bool_typed HAVING COUNT(*)",
        "SELECT COUNT(*) FROM bool_typed HAVING CAST('no' AS TEXT)",
        "WITH c AS (SELECT n FROM bool_empty) SELECT n FROM c WHERE n",
        "SELECT n FROM (SELECT n FROM bool_empty) AS derived WHERE n",
        "WITH c AS (SELECT n FROM bool_empty) SELECT c.n FROM c JOIN bool_typed ON c.n",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in statements.into_iter().zip(results) {
        assert_static_type_error(&result, sql);
    }
}

#[test]
fn should_coerce_unknown_no_to_false_without_coercing_typed_text() {
    // Arrange
    let fixture = typed_fixture("bool-unknown-no");

    // Act
    let filtered = fixture.rows("SELECT 1 WHERE 'no'");
    let operators = fixture.rows("SELECT TRUE AND 'no', FALSE OR 'no', NOT 'no'");
    let searched_case = fixture.rows("SELECT CASE WHEN 'no' THEN 1 ELSE 2 END");
    let typed_text = fixture.execute("SELECT 1 WHERE CAST('no' AS TEXT)");

    // Assert
    assert_eq!(filtered, Vec::<Vec<Value>>::new());
    assert_eq!(
        operators,
        vec![vec![
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true)
        ]]
    );
    assert_eq!(searched_case, vec![vec![Value::Int64(2)]]);
    assert_static_type_error(&typed_text, "SELECT 1 WHERE CAST('no' AS TEXT)");
}

#[test]
fn should_contextualize_unknown_literals_at_relation_predicate_roots() {
    // Arrange
    let fixture = typed_fixture("bool-join-having-unknown-input");
    fixture
        .execute("CREATE TABLE bool_rhs (other INT)")
        .expect("create nonempty right input");
    fixture
        .execute("INSERT INTO bool_rhs (other) VALUES (1)")
        .expect("seed right input");

    // Act
    let inner = fixture
        .rows("SELECT bool_typed.n FROM bool_typed JOIN bool_rhs ON 'no' ORDER BY bool_typed.n");
    let left = fixture.rows(
        "SELECT bool_typed.n, bool_rhs.other FROM bool_typed LEFT JOIN bool_rhs ON 'no' ORDER BY bool_typed.n",
    );
    let rejected_group = fixture.rows("SELECT COUNT(*) FROM bool_typed HAVING 'no'");
    let selected_group = fixture.rows("SELECT COUNT(*) FROM bool_typed HAVING 'yes'");

    // Assert
    assert_eq!(inner, Vec::<Vec<Value>>::new());
    assert_eq!(
        left,
        vec![
            vec![Value::Int64(1), Value::Null],
            vec![Value::Int64(2), Value::Null],
        ]
    );
    assert_eq!(rejected_group, Vec::<Vec<Value>>::new());
    assert_eq!(selected_group, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_share_canonical_boolean_input_across_sql_adapters() {
    // Arrange
    let fixture = sql_fixture("bool-input-vocabulary", &[]);
    let spellings = [
        ("t", true),
        ("tr", true),
        ("TRUE", true),
        ("y", true),
        ("Ye", true),
        ("yes", true),
        ("on", true),
        ("1", true),
        ("  true  ", true),
        ("f", false),
        ("fa", false),
        ("FALSE", false),
        ("n", false),
        ("no", false),
        ("of", false),
        ("off", false),
        ("0", false),
        ("  NO  ", false),
    ];

    for (text, expected) in spellings {
        // Act
        let cast = fixture.execute(&format!("SELECT CAST('{text}' AS BOOLEAN)"));
        let selected = fixture.execute(&format!("SELECT 1 WHERE '{text}'"));
        let operator = fixture.execute(&format!("SELECT '{text}' AND TRUE"));

        // Assert
        let cast = cast.expect("supported Boolean cast spelling");
        assert_eq!(
            cast.rows,
            vec![vec![Value::Bool(expected)]],
            "cast {text:?}"
        );
        assert_eq!(cast.columns[0].type_oid, 16);
        assert_eq!(
            selected.expect("supported Boolean predicate spelling").rows,
            if expected {
                vec![vec![Value::Int64(1)]]
            } else {
                Vec::new()
            },
            "predicate {text:?}"
        );
        let operator = operator.expect("supported Boolean operator spelling");
        assert_eq!(
            operator.rows,
            vec![vec![Value::Bool(expected)]],
            "AND {text:?}"
        );
        assert_eq!(operator.columns[0].type_oid, 16);
    }
}

#[test]
fn should_reject_invalid_unknown_boolean_input_before_row_evaluation() {
    // Arrange
    let fixture = typed_fixture("bool-invalid-unknown-input");
    let invalid = ["", "o", "nonempty", "truth", "2"];

    for text in invalid {
        let statements = [
            format!("SELECT 1 WHERE '{text}'"),
            format!("SELECT n FROM bool_empty WHERE '{text}'"),
            format!("SELECT n FROM bool_typed WHERE '{text}' LIMIT 0"),
            format!("SELECT FALSE AND '{text}'"),
            format!("SELECT CASE WHEN '{text}' THEN 1 ELSE 2 END"),
            format!("SELECT CAST('{text}' AS BOOLEAN)"),
        ];

        // Act
        let results = statements
            .iter()
            .map(|sql| fixture.execute(sql))
            .collect::<Vec<_>>();

        // Assert
        for (sql, result) in statements.iter().zip(results) {
            assert!(
                result.is_err(),
                "invalid Boolean input must fail: {sql}: {result:?}"
            );
        }
    }
}

#[test]
fn should_preserve_current_case_selection_semantics() {
    // Arrange
    let fixture = sql_fixture("bool-case-boundaries", &[]);

    // Act
    let result = fixture
        .execute(
            "SELECT CASE 'no' WHEN 'no' THEN 'matched' ELSE 'missed' END, \
         CASE WHEN NULL THEN 'wrong' WHEN FALSE THEN 'wrong' ELSE 'chosen' END, \
         CASE WHEN FALSE THEN 1 END, CASE WHEN TRUE THEN 1 ELSE 1 / 0 END",
        )
        .expect("preserve CASE comparison, UNKNOWN, missing ELSE and lazy results");

    // Assert
    assert_eq!(
        result.rows,
        vec![vec![
            Value::String("matched".to_string()),
            Value::String("chosen".to_string()),
            Value::Null,
            Value::Int64(1),
        ]]
    );
    assert_eq!(result.columns[0].type_oid, 25);
    assert_eq!(result.columns[1].type_oid, 25);
}

fn truth_table() -> [(&'static str, &'static str, Value, Value); 9] {
    [
        ("TRUE", "TRUE", Value::Bool(true), Value::Bool(true)),
        ("TRUE", "FALSE", Value::Bool(false), Value::Bool(true)),
        ("TRUE", "NULL", Value::Null, Value::Bool(true)),
        ("FALSE", "TRUE", Value::Bool(false), Value::Bool(true)),
        ("FALSE", "FALSE", Value::Bool(false), Value::Bool(false)),
        ("FALSE", "NULL", Value::Bool(false), Value::Null),
        ("NULL", "TRUE", Value::Null, Value::Bool(true)),
        ("NULL", "FALSE", Value::Bool(false), Value::Null),
        ("NULL", "NULL", Value::Null, Value::Null),
    ]
}

#[test]
fn should_preserve_complete_table_free_three_valued_boolean_truth_tables() {
    // Arrange
    let fixture = sql_fixture("bool-table-free-truth-tables", &[]);

    for (left, right, expected_and, expected_or) in truth_table() {
        // Act
        let result = fixture
            .execute(&format!("SELECT {left} AND {right}, {left} OR {right}"))
            .expect("evaluate Boolean pair");

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![expected_and, expected_or]],
            "{left}, {right}"
        );
        assert!(result.columns.iter().all(|column| column.type_oid == 16));
    }
    for (operand, expected) in [
        ("TRUE", Value::Bool(false)),
        ("FALSE", Value::Bool(true)),
        ("NULL", Value::Null),
    ] {
        // Continue the same truth-table evaluation for NOT.
        let result = fixture
            .execute(&format!("SELECT NOT {operand}"))
            .expect("evaluate NOT");

        // Assert
        assert_eq!(result.rows, vec![vec![expected]]);
        assert_eq!(result.columns[0].type_oid, 16);
    }
}

fn storage_fixture(label: &str, column_store: bool, column_index: bool) -> SqlFixture {
    let suffix = if column_store {
        " WITH (storage = column_store)"
    } else {
        ""
    };
    let create = format!(
        "CREATE TABLE bool_paths (row_key INT, lhs BOOLEAN, rhs BOOLEAN, n INT, txt TEXT){suffix}"
    );
    let fixture = sql_fixture(label, &[create.as_str()]);
    for (row_key, (left, right, _, _)) in truth_table().into_iter().enumerate() {
        fixture.execute(&format!(
            "INSERT INTO bool_paths (row_key, lhs, rhs, n, txt) VALUES ({row_key}, {left}, {right}, 1, 'no')"
        )).expect("seed Boolean pair");
    }
    if column_index {
        fixture.execute(
            "CREATE INDEX bool_paths_idx ON bool_paths USING column (row_key, lhs, rhs, n, txt) WITH (segment_size = 2)"
        ).expect("build covering CBM2 index");
    }
    fixture
}

#[test]
fn should_preserve_three_valued_boolean_semantics_across_current_storage_paths() {
    // Arrange
    for (label, column_store, column_index) in [
        ("bool-row-truth-table", false, false),
        ("bool-column-store-truth-table", true, false),
        ("bool-column-index-truth-table", false, true),
    ] {
        let fixture = storage_fixture(label, column_store, column_index);
        let expected = truth_table()
            .into_iter()
            .map(|(left, _, and, or)| {
                let not = match left {
                    "TRUE" => Value::Bool(false),
                    "FALSE" => Value::Bool(true),
                    _ => Value::Null,
                };
                vec![and, or, not]
            })
            .collect::<Vec<_>>();

        // Act
        let result = fixture
            .execute("SELECT lhs AND rhs, lhs OR rhs, NOT lhs FROM bool_paths ORDER BY row_key")
            .expect("evaluate supported scalar Boolean expressions");
        let conjunction =
            fixture.rows("SELECT row_key FROM bool_paths WHERE lhs AND rhs ORDER BY row_key");
        let disjunction =
            fixture.rows("SELECT row_key FROM bool_paths WHERE lhs OR rhs ORDER BY row_key");
        let unknown = fixture.rows("SELECT row_key FROM bool_paths WHERE NULL");

        // Assert
        assert_eq!(result.rows, expected, "{label}");
        assert!(result.columns.iter().all(|column| column.type_oid == 16));
        assert_eq!(conjunction, vec![vec![Value::Int64(0)]], "{label}");
        assert_eq!(
            disjunction,
            [0, 1, 2, 3, 6].map(|id| vec![Value::Int64(id)]),
            "{label}"
        );
        assert_eq!(unknown, Vec::<Vec<Value>>::new(), "{label}");
    }
}

#[test]
fn should_normalize_boolean_comparison_literals_before_covered_cbm2_scan_selection() {
    // Arrange
    let fixture = storage_fixture("bool-cbm2-literal-coercion", false, true);
    let before = fixture.cassie.metrics();

    // Act
    let result = fixture
        .execute("SELECT row_key, lhs FROM bool_paths WHERE lhs = 'no' ORDER BY row_key")
        .expect("normalize unknown input against declared Boolean column");
    let explain = fixture
        .execute("EXPLAIN SELECT row_key, lhs FROM bool_paths WHERE lhs = 'no' ORDER BY row_key")
        .expect("describe covered Boolean predicate");
    let after = fixture.cassie.metrics();

    // Assert
    assert_eq!(
        result.rows,
        [3, 4, 5].map(|id| vec![Value::Int64(id), Value::Bool(false)])
    );
    assert_eq!(result.columns[1].type_oid, 16);
    let plan = explain_plan_text(&explain);
    assert_explain_contains(plan, "column_batch_index", "bool_paths_idx");
    assert_explain_contains(plan, "encoded_execution", "true");
    assert!(
        after["column_batches"]["scans"]
            .as_u64()
            .expect("scan metric")
            > before["column_batches"]["scans"]
                .as_u64()
                .expect("scan metric")
    );
}

#[test]
fn should_validate_typed_predicates_before_current_storage_path_selection() {
    // Arrange
    for (label, column_store, column_index) in [
        ("bool-row-invalid-path", false, false),
        ("bool-column-store-invalid-path", true, false),
        ("bool-column-index-invalid-path", false, true),
    ] {
        let fixture = storage_fixture(label, column_store, column_index);

        // Act
        let statements = [
            "SELECT row_key FROM bool_paths WHERE n LIMIT 0",
            "SELECT row_key FROM bool_paths WHERE txt",
            "SELECT row_key FROM bool_paths WHERE FALSE AND n",
        ];
        let results = statements.map(|sql| fixture.execute(sql));
        let explain = fixture.execute("EXPLAIN SELECT row_key FROM bool_paths WHERE lhs = FALSE");

        // Assert
        for (sql, result) in statements.into_iter().zip(results) {
            assert_static_type_error(&result, sql);
        }
        let explain = explain.expect("supported typed Boolean predicate");
        let plan = explain_plan_text(&explain);
        if column_store {
            assert_explain_contains(plan, "storage_mode", "column-store");
        }
        if column_index {
            assert_explain_contains(plan, "column_batch_index", "bool_paths_idx");
            assert_explain_contains(plan, "encoded_execution", "true");
        }
    }
}

#[test]
fn should_treat_embedded_string_parameters_as_typed_text_in_boolean_contexts() {
    // Arrange
    let fixture = typed_fixture("bool-embedded-parameter-types");
    let statements = [
        "SELECT 1 WHERE $1",
        "SELECT NOT $1",
        "SELECT $1 AND TRUE",
        "SELECT n FROM bool_empty WHERE $1",
    ];

    for sql in statements {
        // Act
        let string = fixture.cassie.execute_sql(
            &fixture.session,
            sql,
            vec![Value::String("no".to_string())],
        );
        let integer = fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::Int64(1)]);

        // Assert
        assert!(string.is_err(), "embedded String is typed TEXT: {sql}");
        assert!(integer.is_err(), "embedded Int64 is typed numeric: {sql}");
    }
    // Evaluate the accepted values under the same embedded-parameter contract.
    let false_value = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT 1 WHERE $1",
            vec![Value::Bool(false)],
        )
        .expect("typed Boolean false");
    let null_value = fixture
        .cassie
        .execute_sql(&fixture.session, "SELECT 1 WHERE $1", vec![Value::Null])
        .expect("NULL predicate");

    // Assert
    assert_eq!(false_value.rows, Vec::<Vec<Value>>::new());
    assert_eq!(null_value.rows, Vec::<Vec<Value>>::new());
}

#[test]
fn should_preserve_explicit_embedded_parameter_casts_to_boolean() {
    // Arrange
    let fixture = typed_fixture("bool-embedded-explicit-cast-types");
    let cases = [
        (Value::Int64(0), false),
        (Value::Int64(1), true),
        (Value::Int64(-2), true),
        (Value::Bool(false), false),
        (Value::String("no".to_string()), false),
    ];

    for (parameter, expected) in cases {
        // Act
        let result = fixture.cassie.execute_sql(
            &fixture.session,
            "SELECT CAST($1 AS BOOLEAN) AS flag",
            vec![parameter],
        );

        // Assert
        let result = result.expect("explicit conversion retains the source parameter type");
        assert_eq!(result.rows, vec![vec![Value::Bool(expected)]]);
        assert_eq!(result.columns[0].type_oid, 16);
    }
}

#[test]
fn should_preserve_embedded_boolean_comparison_parameter_provenance() {
    // Arrange
    let fixture = typed_fixture("bool-embedded-comparison-parameter-types");
    let accepted = [
        (
            "SELECT $1 = 'no' AS matched",
            Value::Bool(false),
            Value::Bool(true),
        ),
        (
            "SELECT 'no' = $1 AS matched",
            Value::Bool(false),
            Value::Bool(true),
        ),
        (
            "SELECT $1 = 'no' AS matched",
            Value::String("no".to_string()),
            Value::Bool(true),
        ),
        ("SELECT $1 = FALSE AS matched", Value::Null, Value::Null),
    ];
    let rejected = [
        (
            "SELECT $1 = CAST('no' AS TEXT) AS matched",
            Value::Bool(false),
        ),
        ("SELECT $1 = 1 AS matched", Value::Bool(false)),
        (
            "SELECT $1 = FALSE AS matched",
            Value::String("no".to_string()),
        ),
        (
            "SELECT n FROM bool_empty WHERE flag = $1",
            Value::String("no".to_string()),
        ),
        (
            "SELECT n FROM bool_empty WHERE $1 = txt",
            Value::Bool(false),
        ),
    ];

    // Act
    let accepted_results = accepted
        .into_iter()
        .map(|(sql, parameter, expected)| {
            (
                sql,
                expected,
                fixture
                    .cassie
                    .execute_sql(&fixture.session, sql, vec![parameter]),
            )
        })
        .collect::<Vec<_>>();
    let rejected_results = rejected
        .into_iter()
        .map(|(sql, parameter)| {
            (
                sql,
                fixture
                    .cassie
                    .execute_sql(&fixture.session, sql, vec![parameter]),
            )
        })
        .collect::<Vec<_>>();

    // Assert
    for (sql, expected, result) in accepted_results {
        let result = result.expect("same-type or contextual unknown-literal comparison");
        assert_eq!(result.rows, vec![vec![expected]], "{sql}");
        assert_eq!(result.columns[0].type_oid, 16, "{sql}");
    }
    for (sql, result) in rejected_results {
        assert_static_type_error(&result, sql);
    }
}

#[test]
fn should_validate_lateral_outer_boolean_types_before_row_evaluation() {
    // Arrange
    let fixture = sql_fixture(
        "bool-lateral-static-types",
        &[
            "CREATE TABLE bool_lateral_outer (row_key INT, n INT, flag BOOLEAN)",
            "INSERT INTO bool_lateral_outer VALUES (1, 10, TRUE), (2, 20, FALSE)",
            "CREATE TABLE bool_lateral_empty (row_key INT, n INT, flag BOOLEAN)",
            "CREATE TABLE bool_lateral_inner (owner_key INT)",
            "INSERT INTO bool_lateral_inner VALUES (1), (2)",
        ],
    );
    let statements = [
        "SELECT bool_lateral_outer.row_key FROM bool_lateral_outer JOIN LATERAL (SELECT owner_key FROM bool_lateral_inner WHERE bool_lateral_inner.owner_key = bool_lateral_outer.row_key AND bool_lateral_outer.n) AS q ON TRUE LIMIT 0",
        "SELECT bool_lateral_empty.row_key FROM bool_lateral_empty JOIN LATERAL (SELECT owner_key FROM bool_lateral_inner WHERE bool_lateral_inner.owner_key = bool_lateral_empty.row_key AND bool_lateral_empty.n) AS q ON TRUE",
    ];

    // Act
    let control = fixture.execute("SELECT bool_lateral_outer.row_key FROM bool_lateral_outer JOIN LATERAL (SELECT owner_key FROM bool_lateral_inner WHERE bool_lateral_inner.owner_key = bool_lateral_outer.row_key AND bool_lateral_outer.flag) AS q ON TRUE ORDER BY bool_lateral_outer.row_key");
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    assert_eq!(
        control
            .expect("supported LATERAL Boolean context distinguishes the outer rows")
            .rows,
        vec![vec![Value::Int64(1)]]
    );
    for (sql, result) in statements.into_iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "outer typed INT must reject before an empty lateral execution: {sql}: {result:?}"
        );
    }
}

#[test]
fn should_contextualize_boolean_between_inputs_before_storage_path_selection() {
    // Arrange
    for (label, column_store, column_index) in [
        ("bool-between-row", false, false),
        ("bool-between-column-store", true, false),
        ("bool-between-column-index", false, true),
    ] {
        let fixture = storage_fixture(label, column_store, column_index);

        // Act
        let literal = fixture.execute("SELECT 'no' BETWEEN FALSE AND TRUE AS matched");
        let range = fixture.execute(
            "SELECT row_key FROM bool_paths WHERE lhs BETWEEN 'no' AND 'yes' ORDER BY row_key",
        );
        let typed_text = fixture.cassie.execute_sql(
            &fixture.session,
            "SELECT $1 BETWEEN FALSE AND TRUE AS matched FROM bool_paths LIMIT 0",
            vec![Value::String("no".to_string())],
        );
        let null = fixture.cassie.execute_sql(
            &fixture.session,
            "SELECT $1 BETWEEN FALSE AND TRUE AS matched",
            vec![Value::Null],
        );

        // Assert
        assert_eq!(
            literal
                .expect("unknown literal gets Boolean range context")
                .rows,
            vec![vec![Value::Bool(true)]],
            "{label}"
        );
        assert_eq!(
            range
                .expect("Boolean range bounds normalize before scan selection")
                .rows,
            (0..6).map(|id| vec![Value::Int64(id)]).collect::<Vec<_>>(),
            "{label}"
        );
        assert!(
            matches!(typed_text, Err(CassieError::Planner(_))),
            "typed TEXT range input: {label}"
        );
        assert_eq!(
            null.expect("SQL NULL range expression").rows,
            vec![vec![Value::Null]],
            "{label}"
        );
    }
}

#[test]
fn should_preserve_exported_lateral_boolean_output_types() {
    // Arrange
    let fixture = sql_fixture(
        "bool-lateral-exported-output-types",
        &[
            "CREATE TABLE bool_lateral_output (row_key INT, n INT, flag BOOLEAN)",
            "INSERT INTO bool_lateral_output VALUES (1, 10, TRUE), (2, 20, FALSE)",
            "CREATE TABLE bool_lateral_shadow (flag INT)",
            "INSERT INTO bool_lateral_shadow VALUES (7)",
        ],
    );

    // Act
    let boolean = fixture.execute("SELECT bool_lateral_output.row_key FROM bool_lateral_output JOIN LATERAL (SELECT bool_lateral_output.flag AS flag) AS q ON q.flag ORDER BY bool_lateral_output.row_key");
    let integer = fixture.execute("SELECT bool_lateral_output.row_key FROM bool_lateral_output JOIN LATERAL (SELECT bool_lateral_output.n AS flag) AS q ON q.flag LIMIT 0");
    let shadow = fixture.execute("SELECT bool_lateral_output.row_key FROM bool_lateral_output JOIN LATERAL (SELECT flag FROM bool_lateral_shadow) AS q ON q.flag LIMIT 0");
    let qualified = fixture.execute("SELECT bool_lateral_output.row_key FROM bool_lateral_output JOIN LATERAL (SELECT bool_lateral_output.flag AS flag FROM bool_lateral_shadow) AS q ON q.flag ORDER BY bool_lateral_output.row_key");

    // Assert
    assert_eq!(
        boolean
            .expect("a supported LATERAL BOOLEAN output stays BOOLEAN at the parent ON sink")
            .rows,
        vec![vec![Value::Int64(1)]]
    );
    assert!(
        matches!(integer, Err(CassieError::Planner(_))),
        "a typed INT output must reject before LIMIT 0"
    );
    assert!(
        matches!(shadow, Err(CassieError::Planner(_))),
        "inner unqualified INT wins over outer BOOLEAN"
    );
    assert_eq!(
        qualified
            .expect("qualified outer BOOLEAN keeps its type despite an inner same-name INT")
            .rows,
        vec![vec![Value::Int64(1)]]
    );
}
