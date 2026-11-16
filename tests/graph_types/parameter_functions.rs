//! A bound parameter declared with a reserved type reaches function arguments as that type,
//! in text and binary form (#1268).

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

type Setup = Vec<(&'static str, Vec<cassie::types::Value>)>;

fn social() -> Setup {
    crate::fixture::SOCIAL_SETUP
        .iter()
        .map(|sql| (*sql, Vec::new()))
        .collect()
}

fn type_oid(name: &str) -> i32 {
    let sql = format!("SELECT oid::text FROM pg_type WHERE typname = '{name}'");
    let batches = fixture::run_cycles(
        &format!("param-fn-oid-{name}"),
        &social(),
        vec![fixture::execute_cycle(&sql, None, 0, false)],
    );
    wire::data_rows(&batches[0])[0][0]
        .as_deref()
        .and_then(|oid| oid.parse().ok())
        .expect("type OID")
}

/// The wire form (`text` or `binary`) of a value expression, as a bind parameter body.
fn body_of(expression: &str, format: i16) -> Vec<u8> {
    let sql = format!("SELECT {expression}");
    let batches = fixture::run_cycles(
        "param-fn-body",
        &social(),
        vec![fixture::execute_cycle(&sql, None, format, false)],
    );
    if format == 0 {
        wire::data_rows(&batches[0])[0][0]
            .clone()
            .expect("a value")
            .into_bytes()
    } else {
        // `DataRow` payload: field count (2 bytes), field length (4 bytes), field.
        fixture::data_row_payloads(&batches[0])[0][6..].to_vec()
    }
}

fn run_bound(label: &str, sql: &str, oid: i32, format: i16, body: &[u8]) -> fixture::Frames {
    let cycles = vec![fixture::execute_cycle(
        sql,
        Some((oid, format, Some(body))),
        0,
        false,
    )];
    fixture::run_cycles(label, &social(), cycles).remove(0)
}

fn path_expression() -> String {
    crate::fixture::one_hop(2, 9_007_199_254_740_993, 1)
}

fn first_cell(frames: &fixture::Frames) -> String {
    fixture::assert_success(frames);
    wire::data_rows(frames)[0][0].clone().expect("a value")
}

fn path_to_jsonb(label: &str, format: i16) -> String {
    let oid = type_oid("graph_path");
    let body = body_of(&path_expression(), format);
    let frames = run_bound(
        label,
        "SELECT cassie_graph.path_to_jsonb($1)::text",
        oid,
        format,
        &body,
    );
    first_cell(&frames)
}

#[test]
fn should_pass_a_text_path_parameter_to_path_to_jsonb() {
    // Arrange
    let label = "param-fn-path-text";

    // Act
    let document = path_to_jsonb(label, 0);

    // Assert
    assert!(document.contains("9007199254740993"));
    assert!(document.contains("\"directions\": [1]"));
}

#[test]
fn should_pass_a_binary_path_parameter_to_path_to_jsonb() {
    // Arrange
    let label = "param-fn-path-binary";

    // Act
    let document = path_to_jsonb(label, 1);

    // Assert
    assert!(document.contains("9007199254740993"));
    assert!(document.contains("\"directions\": [1]"));
}

#[test]
fn should_pass_a_text_path_parameter_to_path_steps() {
    // Arrange
    let oid = type_oid("graph_path");
    let body = body_of(&path_expression(), 0);

    // Act
    let frames = run_bound(
        "param-fn-steps-text",
        "SELECT count(*)::text FROM cassie_graph.path_steps($1)",
        oid,
        0,
        &body,
    );

    // Assert
    assert_eq!(first_cell(&frames), "2");
}

#[test]
fn should_pass_a_binary_path_parameter_to_path_steps() {
    // Arrange
    let oid = type_oid("graph_path");
    let body = body_of(&path_expression(), 1);

    // Act
    let frames = run_bound(
        "param-fn-steps-binary",
        "SELECT count(*)::text FROM cassie_graph.path_steps($1)",
        oid,
        1,
        &body,
    );

    // Assert
    assert_eq!(first_cell(&frames), "2");
}

#[test]
fn should_pass_a_text_reference_parameter_to_key_parts() {
    // Arrange
    let oid = type_oid("graph_element_ref");
    let body = body_of(&crate::fixture::person(9_007_199_254_740_993), 0);

    // Act
    let frames = run_bound(
        "param-fn-key-parts-text",
        "SELECT k.integer_value::text FROM cassie_graph.key_parts($1) AS k",
        oid,
        0,
        &body,
    );

    // Assert
    assert_eq!(first_cell(&frames), "9007199254740993");
}

#[test]
fn should_pass_a_binary_reference_parameter_to_key_parts() {
    // Arrange
    let oid = type_oid("graph_element_ref");
    let body = body_of(&crate::fixture::person(9_007_199_254_740_993), 1);

    // Act
    let frames = run_bound(
        "param-fn-key-parts-binary",
        "SELECT k.integer_value::text FROM cassie_graph.key_parts($1) AS k",
        oid,
        1,
        &body,
    );

    // Assert
    assert_eq!(first_cell(&frames), "9007199254740993");
}

#[test]
fn should_infer_the_path_type_of_an_undeclared_parameter_given_path_to_jsonb() {
    // Arrange
    let body = body_of(&path_expression(), 0);

    // Act
    let frames = run_bound(
        "param-fn-infer-jsonb",
        "SELECT cassie_graph.path_to_jsonb($1)::text",
        0,
        0,
        &body,
    );

    // Assert
    assert!(first_cell(&frames).contains("9007199254740993"));
}

#[test]
fn should_infer_the_path_type_of_an_undeclared_binary_parameter_given_path_to_jsonb() {
    // Arrange
    let body = body_of(&path_expression(), 1);

    // Act
    let frames = run_bound(
        "param-fn-infer-jsonb-binary",
        "SELECT cassie_graph.path_to_jsonb($1)::text",
        0,
        1,
        &body,
    );

    // Assert
    assert!(first_cell(&frames).contains("9007199254740993"));
}

#[test]
fn should_infer_the_path_type_of_an_undeclared_parameter_given_path_steps() {
    // Arrange
    let body = body_of(&path_expression(), 0);

    // Act
    let frames = run_bound(
        "param-fn-infer-steps",
        "SELECT count(*)::text FROM cassie_graph.path_steps($1)",
        0,
        0,
        &body,
    );

    // Assert
    assert_eq!(first_cell(&frames), "2");
}

#[test]
fn should_infer_the_reference_type_of_an_undeclared_parameter_given_key_parts() {
    // Arrange
    let body = body_of(&crate::fixture::person(9_007_199_254_740_993), 0);

    // Act
    let frames = run_bound(
        "param-fn-infer-key-parts",
        "SELECT k.integer_value::text FROM cassie_graph.key_parts($1) AS k",
        0,
        0,
        &body,
    );

    // Assert
    assert_eq!(first_cell(&frames), "9007199254740993");
}
