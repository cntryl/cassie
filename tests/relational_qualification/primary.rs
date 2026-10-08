use crate::support_relational_qualification::{assert_seed_carriers, fixture, records};
use cassie::types::Value;

fn matches_literal(actual: &Value, expected: &serde_json::Value) -> bool {
    match actual {
        Value::Null => expected.is_null(),
        Value::Bool(value) => expected.as_bool() == Some(*value),
        Value::Int64(value) => expected.as_i64() == Some(*value),
        Value::Float64(value) => expected.as_f64() == Some(*value),
        Value::String(value) => expected.as_str() == Some(value.as_str()),
        Value::Json(value) => value == expected,
        Value::Vector(_) => false,
    }
}

fn matches_row(actual: &[Value], expected: &[serde_json::Value]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| matches_literal(actual, expected))
}

#[test]
fn should_preserve_declared_seed_carriers() {
    // Arrange
    let fixture = fixture();

    // Act
    assert_seed_carriers(&fixture);

    // Assert
    assert_eq!(records().seeds.len(), 14);
}

#[test]
fn should_match_independent_relational_truth_tables() {
    // Arrange
    let fixture = fixture();
    let records = records();
    assert_eq!(records.primary.len(), 22);
    assert_eq!(records.variants.len(), 20);

    // Act
    for case in records.primary {
        let result = fixture
            .cassie
            .execute_sql(&fixture.session, &case.sql, vec![]);

        // Assert
        if let Some(error) = case.error {
            let actual = result.expect_err("selected unsupported syntax");
            let fragment = error["message_fragment"]
                .as_str()
                .expect("literal error fragment");
            assert!(
                actual.to_string().contains(fragment),
                "{}: {actual}",
                case.invariant
            );
            assert!(
                matches!(actual, cassie::app::CassieError::Unsupported(_)),
                "{}: selected unsupported error: {actual:?}",
                case.invariant
            );
            println!("{} selected error: {actual}", case.invariant);
            continue;
        }
        let result = result.unwrap_or_else(|error| panic!("{}: {error}", case.invariant));
        let columns = case.columns.expect("literal descriptor");
        assert_eq!(result.columns.len(), columns.len(), "{}", case.invariant);
        for (actual, expected) in result.columns.iter().zip(columns) {
            // Embedded execution uses its own projection labels; pgwire names are checked separately.
            assert_eq!(
                (
                    actual.type_oid,
                    actual.typlen,
                    actual.atttypmod,
                    actual.format_code
                ),
                (i64::from(expected.oid), expected.typlen, expected.typmod, 0),
                "{}",
                case.invariant
            );
        }
        let expected = case.expected_rows.expect("literal rows");
        assert_eq!(
            result.rows.len(),
            expected.len(),
            "{}: {:?}",
            case.invariant,
            result.rows
        );
        println!(
            "{} rows={:?} columns={:?}",
            case.invariant, result.rows, result.columns
        );
        if case.ordered {
            assert!(
                result
                    .rows
                    .iter()
                    .zip(&expected)
                    .all(|(actual, expected)| matches_row(actual, expected)),
                "{} ordered rows",
                case.invariant
            );
        } else {
            let mut remaining = result.rows;
            for row in expected {
                let index = remaining
                    .iter()
                    .position(|actual| matches_row(actual, &row))
                    .unwrap_or_else(|| panic!("{} missing bag member {row:?}", case.invariant));
                remaining.remove(index);
            }
            assert_eq!(remaining, Vec::<Vec<Value>>::new());
        }
    }
}

#[test]
fn should_preserve_primary_wire_descriptors() {
    // Arrange
    use crate::support_relational_qualification::{cycle, run_wire};
    let fixture = fixture();
    let records = records();
    let cycles = records
        .primary
        .iter()
        .flat_map(|case| [cycle(&case.sql, &[], 0), cycle(&case.sql, &[], 1)])
        .collect();

    // Act
    let results = run_wire(&fixture, cycles);

    // Assert
    assert_eq!(results.len(), 44);
    for (case, pair) in records
        .primary
        .iter()
        .zip(results.as_chunks::<2>().0.iter())
    {
        for (format, frames) in [0, 1].into_iter().zip(pair) {
            if let Some(error) = &case.error {
                assert_eq!(
                    crate::support_pgwire::error_code(frames).as_deref(),
                    error["sqlstate"].as_str(),
                    "{}",
                    case.invariant
                );
                assert!(
                    !frames.iter().any(|(tag, _)| *tag == b'T'),
                    "{} no descriptor on parse error",
                    case.invariant
                );
                continue;
            }
            assert_eq!(
                crate::support_pgwire::error_code(frames),
                None,
                "{}",
                case.invariant
            );
            let descriptions = frames
                .iter()
                .filter(|(tag, _)| *tag == b'T')
                .map(|(_, payload)| crate::support_pgwire::parse_row_description(payload))
                .collect::<Vec<_>>();
            assert!(
                descriptions.len() >= 2,
                "{} Statement and Portal descriptions",
                case.invariant
            );
            for (index, description) in descriptions.iter().enumerate() {
                let expected = case.columns.as_ref().expect("literal wire descriptor");
                assert_eq!(description.len(), expected.len(), "{}", case.invariant);
                for (actual, expected) in description.iter().zip(expected) {
                    assert_eq!(
                        (
                            &actual.name,
                            actual.type_oid,
                            actual.type_size,
                            actual.type_mod,
                            actual.format_code
                        ),
                        (
                            &expected.name,
                            expected.oid,
                            expected.typlen,
                            expected.typmod,
                            if index == 0 { 0 } else { format }
                        ),
                        "{}",
                        case.invariant
                    );
                }
            }
        }
    }
}

#[test]
fn should_evaluate_correlated_exists_in_the_selected_output() {
    // Arrange
    let fixture = fixture();
    let cases = [
        ("ordinary_alias", "SELECT o.id,EXISTS(SELECT i.id FROM r AS i WHERE i.id=o.id AND i.n>=0) AS present FROM r AS o ORDER BY o.id"),
        ("unaliased_outer", "SELECT r.id,EXISTS(SELECT i.id FROM r AS i WHERE i.id=r.id AND i.n>=0) AS present FROM r ORDER BY r.id"),
        ("declared_alias_columns", "SELECT o.oid,EXISTS(SELECT i.id FROM r AS i WHERE i.id=o.oid AND i.n>=0) AS present FROM r AS o(oid,g,n,f,label,a) ORDER BY o.oid"),
    ];
    let expected = [[1, 1], [2, 1], [3, 0], [4, 0], [5, 1], [6, 1]];

    // Act
    let results = cases.map(|(name, sql)| {
        (
            name,
            fixture.cassie.execute_sql(&fixture.session, sql, vec![]),
        )
    });
    let filter = fixture.cassie.execute_sql(&fixture.session,
        "SELECT o.id FROM r AS o WHERE EXISTS(SELECT i.id FROM r AS i WHERE i.id=o.id AND i.n>=0) ORDER BY o.id", vec![]);
    for (name, result) in &results {
        println!("{name}: {result:?}");
    }
    println!("ordinary_alias_where_control: {filter:?}");

    // Assert
    assert_eq!(
        filter.expect("existing correlated WHERE").rows,
        vec![
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(5)],
            vec![Value::Int64(6)]
        ]
    );
    for (name, result) in results {
        let result = result.unwrap_or_else(|error| panic!("{name}: {error}"));
        let expected =
            expected.map(|[id, present]| vec![Value::Int64(id), Value::Bool(present == 1)]);
        assert_eq!(result.rows, expected, "{name}");
    }
}

#[test]
fn should_preserve_recursive_window_truth_tables() {
    // Arrange
    let fixture = fixture();
    let cases = records()
        .primary
        .into_iter()
        .filter(|case| ["SQL-024", "SQL-025"].contains(&case.invariant.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 2);

    // Act
    let results = cases
        .iter()
        .map(|case| {
            fixture
                .cassie
                .execute_sql(&fixture.session, &case.sql, vec![])
        })
        .collect::<Vec<_>>();

    // Assert
    for (case, result) in cases.iter().zip(results) {
        let result = result.unwrap_or_else(|error| panic!("{}: {error}", case.invariant));
        let expected = case.expected_rows.as_ref().expect("literal truth table");
        assert_eq!(result.rows.len(), expected.len(), "{}", case.invariant);
        assert!(
            result
                .rows
                .iter()
                .zip(expected)
                .all(|(actual, expected)| matches_row(actual, expected)),
            "{} rows={:?}",
            case.invariant,
            result.rows
        );
        let columns = case.columns.as_ref().expect("literal descriptor");
        assert_eq!(result.columns.len(), columns.len());
        for (actual, expected) in result.columns.iter().zip(columns) {
            assert_eq!(
                (
                    &actual.name,
                    actual.type_oid,
                    actual.typlen,
                    actual.atttypmod
                ),
                (
                    &expected.name,
                    i64::from(expected.oid),
                    expected.typlen,
                    expected.typmod
                ),
                "{}",
                case.invariant
            );
        }
        println!(
            "{} rows={:?} columns={:?}",
            case.invariant, result.rows, result.columns
        );
    }
}

#[test]
fn should_preserve_uncorrelated_output_folding() {
    // Arrange
    let fixture = fixture();
    let empty_sql = "SELECT id,EXISTS(SELECT id FROM r WHERE n>=0) AS present FROM r WHERE id<0";
    let error_sql = "SELECT EXISTS(SELECT n / n FROM r WHERE id=5) AS bad FROM r WHERE id<0";
    let mixed_sql = "SELECT EXISTS(SELECT n / n FROM r WHERE id=5) AS bad,EXISTS(SELECT i.id FROM r AS i WHERE i.id=o.id) AS present FROM r AS o WHERE o.id<0";

    // Act
    let output = fixture.cassie.execute_sql(
        &fixture.session,
        "SELECT id,EXISTS(SELECT id FROM r WHERE n>=0) AS present FROM r ORDER BY id",
        vec![],
    );
    let empty = fixture
        .cassie
        .execute_sql(&fixture.session, empty_sql, vec![]);
    let error = fixture
        .cassie
        .execute_sql(&fixture.session, error_sql, vec![]);
    let mixed = fixture
        .cassie
        .execute_sql(&fixture.session, mixed_sql, vec![]);

    // Assert
    assert_eq!(
        output.expect("uncorrelated selected output").rows,
        [1, 2, 3, 4, 5, 6].map(|id| vec![Value::Int64(id), Value::Bool(true)])
    );
    assert_eq!(
        empty.expect("empty uncorrelated output").rows,
        [] as [Vec<Value>; 0]
    );
    for (name, result) in [
        ("empty-input eager error", error),
        ("uncorrelated error before correlated sibling", mixed),
    ] {
        let error = result.expect_err("existing source-independent folding error");
        assert!(
            matches!(error, cassie::app::CassieError::Execution(ref message) if message.contains("division by zero")),
            "{name}: {error:?}"
        );
        println!("{name}: {error:?}");
    }
}

#[test]
fn should_preserve_inner_scope_for_exists_output() {
    // Arrange
    let fixture = fixture();
    let inner_shadow = "SELECT o.id,EXISTS(SELECT o.id FROM r AS o WHERE o.id=3) AS present FROM r AS o ORDER BY o.id";
    let correlated_case = "SELECT o.id,CASE WHEN o.id=3 THEN false ELSE EXISTS(SELECT i.id FROM r AS i WHERE i.id=o.id AND i.n>=0) END AS present FROM r AS o ORDER BY o.id";
    let correlated_function = "SELECT o.id,COALESCE(EXISTS(SELECT i.id FROM r AS i WHERE i.id=o.id AND i.n>=0),false) AS present FROM r AS o ORDER BY o.id";

    // Act
    let shadow = fixture
        .cassie
        .execute_sql(&fixture.session, inner_shadow, vec![]);
    let nested = [correlated_case, correlated_function]
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]));
    for (sql, result) in [correlated_case, correlated_function].iter().zip(&nested) {
        println!("{sql}: {result:?}");
    }

    // Assert
    assert_eq!(
        shadow.expect("inner alias shadows outer alias").rows,
        [1, 2, 3, 4, 5, 6].map(|id| vec![Value::Int64(id), Value::Bool(true)])
    );
    let expected = [
        (1, true),
        (2, true),
        (3, false),
        (4, false),
        (5, true),
        (6, true),
    ]
    .map(|(id, present)| vec![Value::Int64(id), Value::Bool(present)]);
    for result in nested {
        assert_eq!(result.expect("selected correlated wrapper").rows, expected);
    }
}

#[test]
fn should_preserve_occurrence_scope_for_exists_output() {
    // Arrange
    let fixture = fixture();
    let cases = [
        "WITH c AS (SELECT id,n FROM r) SELECT o.id,EXISTS(SELECT i.id FROM c AS i WHERE i.id=o.id AND i.n>=0) AS present FROM c AS o ORDER BY o.id",
        "WITH c AS (SELECT id,n FROM r) SELECT c.id,EXISTS(SELECT i.id FROM c AS i WHERE i.id=c.id AND i.n>=0) AS present FROM c ORDER BY c.id",
        "SELECT o.id,EXISTS(SELECT id FROM r WHERE id=0) OR EXISTS(SELECT i.id FROM r AS i WHERE i.id=o.id AND i.n>=0) AS present FROM r AS o ORDER BY o.id",
    ];
    let local_shadow = "SELECT o.id,EXISTS(WITH o AS (SELECT id FROM r WHERE id=3) SELECT id FROM o WHERE id=3) AS present FROM r AS o ORDER BY o.id";
    let eager_error = "SELECT COALESCE(EXISTS(SELECT i.id FROM r AS i WHERE i.id=o.id),EXISTS(SELECT n / n FROM r WHERE id=5)) AS present FROM r AS o WHERE o.id<0";
    let invalid = "SELECT EXISTS(SELECT r.id FROM r AS i WHERE r.id=i.id) AS present FROM r AS o";

    // Act
    let results = cases.map(|sql| {
        (
            sql,
            fixture.cassie.execute_sql(&fixture.session, sql, vec![]),
        )
    });
    let shadow = fixture
        .cassie
        .execute_sql(&fixture.session, local_shadow, vec![]);
    let error = fixture
        .cassie
        .execute_sql(&fixture.session, eager_error, vec![]);
    let invalid = fixture
        .cassie
        .execute_sql(&fixture.session, invalid, vec![]);

    // Assert
    let expected = [
        (1, true),
        (2, true),
        (3, false),
        (4, false),
        (5, true),
        (6, true),
    ]
    .map(|(id, present)| vec![Value::Int64(id), Value::Bool(present)]);
    for (sql, result) in results {
        println!("scope control {sql}: {result:?}");
        assert_eq!(result.expect("occurrence scope").rows, expected, "{sql}");
    }
    assert_eq!(
        shadow.expect("local CTE shadows outer alias").rows,
        [1, 2, 3, 4, 5, 6].map(|id| vec![Value::Int64(id), Value::Bool(true)])
    );
    let error = error.expect_err("uncorrelated sibling still folds on empty input");
    assert!(
        matches!(error,cassie::app::CassieError::Execution(ref message) if message.contains("division by zero")),
        "{error:?}"
    );
    let invalid = invalid.expect_err("hidden inner collection is not an enclosing namespace");
    assert!(
        matches!(invalid,cassie::app::CassieError::Planner(ref message) if message.contains("unknown relation qualifier 'r'")),
        "{invalid:?}"
    );
}

#[test]
fn should_skip_dead_correlated_exists_output() {
    // Arrange
    let fixture = fixture();
    let cases = [
        "SELECT o.id,CASE WHEN o.n=0 THEN false ELSE EXISTS(SELECT i.n / i.n FROM r AS i WHERE i.id=o.id AND i.n>=0) END AS present FROM r AS o WHERE o.id=5",
        "SELECT o.id,COALESCE(true,EXISTS(SELECT i.n / i.n FROM r AS i WHERE i.id=o.id AND i.n>=0)) AS present FROM r AS o WHERE o.id=5",
    ];

    // Act
    let results = cases.map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]));

    for (sql, result) in cases.iter().zip(&results) {
        println!("dead branch observation {sql}: {result:?}");
    }

    // Assert
    for ((sql, result), expected) in cases.into_iter().zip(results).zip([false, true]) {
        println!("dead branch {sql}: {result:?}");
        assert_eq!(
            result
                .expect("dead correlated branch is not evaluated")
                .rows,
            vec![vec![Value::Int64(5), Value::Bool(expected)]],
            "{sql}"
        );
    }
}

#[test]
fn should_preserve_selected_exists_branch_errors() {
    // Arrange
    let fixture = fixture();
    let cases = [
        "SELECT CASE WHEN o.n=0 THEN EXISTS(SELECT i.n / i.n FROM r AS i WHERE i.id=o.id) ELSE false END AS present FROM r AS o WHERE o.id=5",
        "SELECT COALESCE(NULL,EXISTS(SELECT i.n / i.n FROM r AS i WHERE i.id=o.id)) AS present FROM r AS o WHERE o.id=5",
        "SELECT CASE WHEN true THEN false ELSE EXISTS(SELECT n / n FROM r WHERE id=5) END AS present FROM r AS o WHERE o.id=5",
    ];

    // Act
    let results = cases.map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]));

    // Assert
    for (sql, result) in cases.into_iter().zip(results) {
        let error = result.expect_err("selected correlated or eager uncorrelated error");
        println!("selected/eager branch {sql}: {error:?}");
        assert!(
            matches!(error, cassie::app::CassieError::Execution(ref message)
            if message.contains("division by zero")),
            "{sql}: {error:?}"
        );
    }
}

#[test]
fn should_evaluate_correlated_case_operand_once() {
    // Arrange
    let fixture = fixture();
    fixture
        .session
        .set_setting("application_name", "")
        .expect("private session setting");
    let sql = "SELECT CASE set_config('application_name',concat(current_setting('application_name'),'x'),false) WHEN 'x' THEN EXISTS(SELECT i.id FROM r AS i WHERE i.id=o.id) ELSE false END AS present FROM r AS o WHERE o.id=5";

    // Act
    let result = fixture.cassie.execute_sql(&fixture.session, sql, vec![]);
    let setting = fixture.session.setting("application_name");

    // Assert
    println!("once-only CASE {sql}: {result:?} setting={setting:?}");
    assert_eq!(
        result.expect("selected simple CASE").rows,
        vec![vec![Value::Bool(true)]]
    );
    assert_eq!(setting.expect("private session setting"), "x");
}

#[test]
fn should_select_correlated_output_with_boolean_parameters() {
    // Arrange
    let fixture = fixture();
    let cases = [
        "SELECT CASE WHEN $1 THEN false ELSE EXISTS(SELECT i.n / i.n FROM r AS i WHERE i.id=o.id) END AS present FROM r AS o WHERE o.id=5",
        "SELECT COALESCE($1,EXISTS(SELECT i.n / i.n FROM r AS i WHERE i.id=o.id)) AS present FROM r AS o WHERE o.id=5",
    ];

    // Act
    let results = cases.map(|sql| {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::Bool(true)])
    });

    // Assert
    for ((sql, result), expected) in cases.into_iter().zip(results).zip([false, true]) {
        println!("parameter-selected branch {sql}: {result:?}");
        assert_eq!(
            result.expect("selected Boolean parameter").rows,
            vec![vec![Value::Bool(expected)]],
            "{sql}"
        );
    }
}
