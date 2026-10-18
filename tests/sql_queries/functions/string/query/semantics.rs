//! Remaining PostgreSQL 18 query contracts tracked by #803.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_scalar_text_identity_given_unicode_nulls_and_signed_lengths_when_functions_execute(
) {
    // Arrange
    let fixture = sql_fixture("text-scalar-values", &[]);
    let sql = "SELECT char_length('Aé🙂Z'),octet_length('Aé🙂Z'),bit_length('Aé🙂Z'),ascii('🙂'),chr(128578),left('Aé🙂Z',-1),right('Aé🙂Z',-1),starts_with('Aé🙂Z','Aé'),'Aé🙂Z'^@'Aé',num_nulls(1,NULL,'x'),num_nonnulls(1,NULL,'x')";

    // Act
    let result = fixture.execute(sql).expect("scalar text functions");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(4),
            Value::Int64(8),
            Value::Int64(64),
            Value::Int64(128_578),
            Value::String("🙂".into()),
            Value::String("Aé🙂".into()),
            Value::String("é🙂Z".into()),
            Value::Bool(true),
            Value::Bool(true),
            Value::Int64(1),
            Value::Int64(2)
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 23, 23, 25, 25, 25, 16, 16, 23, 23]
    );
}

#[test]
fn should_bound_signed_text_slices_given_extreme_counts_when_utf8_boundaries_are_selected() {
    // Arrange
    let fixture = sql_fixture("text-scalar-boundaries", &[]);
    let sql = "SELECT left('é🙂',2147483647),right('é🙂',2147483647),left('é🙂',-2147483648),right('é🙂',-2147483648),left('é🙂',0),right('é🙂',0),ascii(''),left(NULL::TEXT,1),starts_with(NULL::TEXT,'x')";

    // Act
    let result = fixture.execute(sql).expect("finite signed slicing");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("é🙂".into()),
            Value::String("é🙂".into()),
            Value::String(String::new()),
            Value::String("é🙂".into()),
            Value::String(String::new()),
            Value::String(String::new()),
            Value::Int64(0),
            Value::Null,
            Value::Null
        ]]
    );
}

#[test]
fn should_parse_ident_in_non_strict_mode_before_trailing_text() {
    // Arrange
    let fixture = sql_fixture("issue-803-parse-ident-non-strict", &[]);

    // Act
    let result = fixture.execute("SELECT parse_ident('alpha.beta trailing', false)");

    // Assert
    let result = result.expect("parse_ident non-strict mode accepts trailing text");
    assert_eq!(result.columns()[0].type_oid, 1009);
    assert_eq!(
        result.rows(),
        &[vec![Value::String("{alpha,beta}".to_owned())]]
    );
}
