//! Shared public inference and binder controls for the selected scalar input foundation.

use super::support_sql_fixture::sql_fixture;
use cassie::sql::{bind, parameter_type_oids_with_catalog, parse_statement};

#[test]
fn should_infer_text_input_signatures_through_the_existing_public_projection() {
    // Arrange
    let fixture = sql_fixture("scalar-text-signature-projection", &[]);
    let queries = [
        "SELECT lower($1) AS value",
        "SELECT lower(COALESCE($1, NULL)) AS value",
        "SELECT COALESCE(lower($1), CAST(NULL AS TEXT)) AS value",
    ];

    // Act
    let observations = queries.map(|sql| {
        let parsed = parse_statement(sql).expect("existing scalar syntax");
        let bound = bind(parsed, &fixture.cassie.catalog).expect("existing scalar binding");
        parameter_type_oids_with_catalog(&bound.statement, &[705], &fixture.cassie.catalog)
    });

    // Assert
    for (sql, observed) in queries.into_iter().zip(observations) {
        assert_eq!(observed, vec![25], "{sql}");
    }
}

#[test]
fn should_infer_coalesce_inputs_from_typed_null_anchors() {
    // Arrange
    let fixture = sql_fixture("scalar-coalesce-signature-projection", &[]);
    let cases = [
        ("SELECT COALESCE($1, NULL) AS value", 25),
        ("SELECT COALESCE($1, CAST(NULL AS BOOLEAN)) AS value", 16),
        ("SELECT COALESCE(CAST(NULL AS INT), $1) AS value", 23),
        ("SELECT COALESCE(CAST(NULL AS BIGINT), $1) AS value", 20),
    ];

    // Act
    let observations = cases.map(|(sql, _)| {
        let parsed = parse_statement(sql).expect("existing COALESCE syntax");
        let bound = bind(parsed, &fixture.cassie.catalog).expect("existing COALESCE binding");
        parameter_type_oids_with_catalog(&bound.statement, &[705], &fixture.cassie.catalog)
    });

    // Assert
    for ((sql, expected), observed) in cases.into_iter().zip(observations) {
        assert_eq!(observed, vec![expected], "{sql}");
    }
}

#[test]
fn should_infer_sql_function_inputs_independently_of_the_return_type() {
    // Arrange
    let fixture = sql_fixture("scalar-user-function-input-projection", &[
        "CREATE FUNCTION signature_flag(x BOOLEAN) RETURNS TEXT AS \"CAST(x AS TEXT)\"",
    ]);
    let parsed = parse_statement("SELECT signature_flag($1) AS value").expect("existing UDF syntax");
    let bound = bind(parsed, &fixture.cassie.catalog).expect("existing UDF binding");

    // Act
    let observed = parameter_type_oids_with_catalog(&bound.statement, &[705], &fixture.cassie.catalog);

    // Assert
    assert_eq!(observed, vec![16], "the BOOLEAN input is distinct from the TEXT return");
}

#[test]
fn should_preserve_explicit_parameter_identities_in_public_inference() {
    // Arrange
    let fixture = sql_fixture("scalar-declared-signature-projection", &[]);
    let parsed = parse_statement("SELECT lower($1) AS value").expect("existing scalar syntax");
    let supplied = [16, 23, 25, 705, 0];

    // Act
    let observations = supplied.map(|oid| {
        parameter_type_oids_with_catalog(&parsed, &[oid], &fixture.cassie.catalog)
    });

    // Assert
    assert_eq!(observations, [vec![16], vec![23], vec![25], vec![25], vec![0]]);
}
