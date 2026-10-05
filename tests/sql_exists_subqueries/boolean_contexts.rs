use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::types::Value;

fn correlated_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE bool_outer (id INT, n INT, txt TEXT, flag BOOLEAN)",
            "INSERT INTO bool_outer VALUES (1, 10, 'no', TRUE), (2, 20, 'yes', FALSE), (3, 30, 'no', NULL)",
            "CREATE TABLE bool_outer_empty (id INT, n INT, txt TEXT, flag BOOLEAN)",
            "CREATE TABLE bool_inner (owner_id INT, n BOOLEAN)",
            "INSERT INTO bool_inner VALUES (1, TRUE), (2, TRUE), (3, TRUE)",
        ],
    )
}

#[test]
fn should_validate_correlated_outer_boolean_types_before_row_evaluation() {
    // Arrange
    let fixture = correlated_fixture("bool-correlated-static-types");
    let cases = [
        "SELECT id FROM bool_outer WHERE EXISTS (SELECT 1 FROM bool_inner WHERE bool_inner.owner_id = bool_outer.id AND bool_outer.n)",
        "SELECT id FROM bool_outer WHERE EXISTS (SELECT 1 FROM bool_inner WHERE bool_inner.owner_id = bool_outer.id AND bool_outer.txt)",
        "SELECT id FROM bool_outer_empty WHERE EXISTS (SELECT 1 FROM bool_inner WHERE bool_inner.owner_id = bool_outer_empty.id AND bool_outer_empty.n)",
        "SELECT id FROM bool_outer WHERE EXISTS (SELECT 1 FROM bool_inner WHERE bool_inner.owner_id = bool_outer.id AND NOT bool_outer.txt) LIMIT 0",
    ];
    // This supported correlated control distinguishes each outer row; a folded
    // or uncorrelated EXISTS would incorrectly retain all three rows.
    assert_eq!(
        fixture.rows("SELECT id FROM bool_outer WHERE EXISTS (SELECT 1 FROM bool_inner WHERE bool_inner.owner_id = bool_outer.id AND bool_outer.flag) ORDER BY id"),
        vec![vec![Value::Int64(1)]]
    );

    // Act
    let results = cases.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in cases.into_iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "static correlated type: {sql}: {result:?}"
        );
    }
}

#[test]
fn should_contextualize_unknown_boolean_literals_inside_correlated_exists() {
    // Arrange
    let fixture = correlated_fixture("bool-correlated-unknown-no");
    let sql = "SELECT id FROM bool_outer WHERE EXISTS (SELECT 1 FROM bool_inner WHERE bool_inner.owner_id = bool_outer.id AND 'no') ORDER BY id";
    assert_eq!(
        fixture.rows("SELECT id FROM bool_outer WHERE EXISTS (SELECT 1 FROM bool_inner WHERE bool_inner.owner_id = bool_outer.id AND TRUE) ORDER BY id"),
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)], vec![Value::Int64(3)]]
    );

    // Act
    let selected = fixture.execute(sql);

    // Assert
    assert_eq!(
        selected.expect("canonical unknown Boolean input").rows,
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_reject_invalid_unknown_boolean_input_before_correlated_row_evaluation() {
    // Arrange
    let fixture = correlated_fixture("bool-correlated-empty-invalid");
    let cases = [
        "SELECT id FROM bool_outer_empty WHERE EXISTS (SELECT 1 FROM bool_inner WHERE bool_inner.owner_id = bool_outer_empty.id AND 'o')",
        "SELECT id FROM bool_outer WHERE EXISTS (SELECT 1 FROM bool_inner WHERE bool_inner.owner_id = bool_outer.id AND 'o') LIMIT 0",
    ];

    // Act
    let results = cases.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in cases.into_iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "invalid correlated input: {sql}: {result:?}"
        );
    }
}

#[test]
fn should_resolve_inner_boolean_columns_before_correlated_outer_numeric_columns() {
    // Arrange
    let fixture = correlated_fixture("bool-correlated-inner-type-shadow");
    let sql = "SELECT id FROM bool_outer WHERE EXISTS (SELECT 1 FROM bool_inner WHERE bool_inner.owner_id = bool_outer.id AND n) ORDER BY id";

    // Act
    let selected = fixture.execute(sql);

    // Assert
    assert_eq!(
        selected
            .expect("unqualified n denotes the inner BOOLEAN column")
            .rows,
        vec![
            vec![Value::Int64(1)],
            vec![Value::Int64(2)],
            vec![Value::Int64(3)]
        ]
    );
}

fn scoped_correlated_fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(label, &[]);
    fixture
        .cassie
        .startup()
        .expect("bootstrap canonical catalog");
    for statement in [
        "CREATE TABLE bool_scope_outer (id INT, n INT, flag BOOLEAN)",
        "INSERT INTO bool_scope_outer VALUES (1, 10, TRUE), (2, 20, FALSE)",
        "CREATE TABLE bool_scope_outer_empty (id INT, n INT, flag BOOLEAN)",
        "CREATE TABLE bool_scope_inner (owner_id INT, n BOOLEAN, flag INT)",
        "INSERT INTO bool_scope_inner VALUES (1, TRUE, 1), (2, TRUE, 1)",
    ] {
        fixture
            .execute(statement)
            .expect("canonical shadow fixture");
    }
    fixture
}

#[test]
fn should_reject_outer_numeric_predicates_across_supported_qualifiers() {
    // Arrange
    let fixture = scoped_correlated_fixture("bool-canonical-outer-number");
    let qualifiers = [
        "postgres.public.bool_scope_outer_empty",
        "public.bool_scope_outer_empty",
        "bool_scope_outer_empty",
    ];

    // Act
    let results = qualifiers.map(|qualifier| {
        fixture.execute(&format!(
            "SELECT id FROM public.bool_scope_outer_empty WHERE EXISTS (SELECT 1 FROM public.bool_scope_inner WHERE {qualifier}.n) LIMIT 0"
        ))
    });

    // Assert
    for (qualifier, result) in qualifiers.into_iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "qualified outer INT must not adopt inner BOOLEAN: {qualifier}: {result:?}"
        );
    }
}

#[test]
fn should_resolve_outer_boolean_predicates_in_supported_correlated_scope() {
    // Arrange
    let fixture = scoped_correlated_fixture("bool-canonical-outer-flag");
    let qualifiers = [
        "postgres.public.bool_scope_outer",
        "public.bool_scope_outer",
        "bool_scope_outer",
    ];

    // Act
    let results = qualifiers.map(|qualifier| {
        fixture.execute(&format!(
            "SELECT id FROM public.bool_scope_outer WHERE EXISTS (SELECT 1 FROM public.bool_scope_inner WHERE bool_scope_inner.owner_id = bool_scope_outer.id AND {qualifier}.flag) ORDER BY id"
        ))
    });
    let inner_shadow = fixture.execute("SELECT id FROM public.bool_scope_outer WHERE EXISTS (SELECT 1 FROM public.bool_scope_inner WHERE bool_scope_inner.owner_id = bool_scope_outer.id AND n) ORDER BY id");

    // Assert
    for (qualifier, result) in qualifiers.into_iter().zip(results) {
        if qualifier == "bool_scope_outer" {
            assert_eq!(
                result.expect("local qualified outer BOOLEAN").rows,
                vec![vec![Value::Int64(1)]]
            );
        } else {
            // The existing correlated executor exposes only local relation aliases.
            // Wider execution scope remains owned by #761; Boolean validation must
            // preserve the type rather than misclassifying this as the inner INT.
            assert!(
                matches!(result, Err(CassieError::Execution(ref error)) if error.contains("unresolvable column reference")),
                "existing qualifier boundary: {qualifier}: {result:?}"
            );
        }
    }
    assert_eq!(
        inner_shadow
            .expect("unqualified n denotes the inner BOOLEAN")
            .rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}
