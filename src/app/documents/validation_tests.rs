//! Document ingress diagnostics use the corresponding PostgreSQL 18.6 SQLSTATE.

use super::{Cassie, CassieError};
use crate::catalog::{canonical_relation_name, DEFAULT_SCHEMA};
use crate::config::CassieRuntimeConfig;
use serde_json::json;

fn document_error(column_type: &str, invalid_value: serde_json::Value) -> CassieError {
    let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
        .expect("in-memory document fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE validation_probe (field {column_type})"),
            Vec::new(),
        )
        .expect("create typed collection");
    let collection =
        canonical_relation_name(&cassie.default_database, DEFAULT_SCHEMA, "validation_probe");
    cassie
        .write_document_for_session(
            Some(&session),
            &collection,
            None,
            json!({"field": invalid_value}),
            true,
            None,
        )
        .expect_err("reject invalid document field")
}

#[test]
fn should_report_invalid_boolean_and_float_document_input_as_typed_input_errors() {
    // Arrange
    let cases = [
        (
            "BOOLEAN",
            "invalid input syntax for type boolean: \"banana\"",
        ),
        (
            "DOUBLE PRECISION",
            "invalid input syntax for type double precision: \"banana\"",
        ),
    ];

    // Act
    let errors = cases.map(|(column_type, _)| document_error(column_type, json!("banana")));

    // Assert
    for (error, (_, diagnostic)) in errors.into_iter().zip(cases) {
        assert_eq!(error.sql_state(), "22P02", "{error}");
        assert!(error.to_string().contains(diagnostic), "{error}");
    }
}

#[test]
fn should_report_a_document_varchar_length_error() {
    // Arrange
    let too_long = json!("four");

    // Act
    let error = document_error("VARCHAR(3)", too_long);

    // Assert
    assert_eq!(error.sql_state(), "22001", "{error}");
    assert!(
        error
            .to_string()
            .contains("value too long for type character varying(3)"),
        "{error}"
    );
}

#[test]
fn should_report_numeric_range_errors_for_oid_and_integer_document_fields() {
    // Arrange
    let oid_overflow = json!(4_294_967_296_u64);
    let int_overflow = json!(2_147_483_648_u64);

    // Act
    let oid = document_error("OID", oid_overflow);
    let integer = document_error("INTEGER", int_overflow);

    // Assert
    assert_eq!(oid.sql_state(), "22003", "{oid}");
    assert_eq!(integer.sql_state(), "22003", "{integer}");
    assert!(!oid.to_string().contains("invalid vector"), "{oid}");
    assert!(!integer.to_string().contains("invalid vector"), "{integer}");
}

#[test]
fn should_keep_the_vector_error_for_a_wrong_length_document_vector() {
    // Arrange
    let short_vector = json!([1.0, 2.0]);

    // Act
    let error = document_error("VECTOR(3)", short_vector);

    // Assert
    assert_eq!(error.sql_state(), "22000", "{error}");
    assert!(error.to_string().contains("invalid vector"), "{error}");
}
