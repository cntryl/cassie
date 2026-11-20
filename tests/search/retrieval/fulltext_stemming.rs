use super::support_sql as support;

use cassie::app::Cassie;
use cassie::types::Value;

fn run(cassie: &Cassie, sql: &str, params: Vec<Value>) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, params)
        .expect("statement should execute")
        .rows()
        .to_vec()
}

fn support_fixture(index_options: &str) -> Cassie {
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    run(
        &cassie,
        "CREATE TABLE kb (id INT NOT NULL, body TEXT)",
        vec![],
    );
    for (id, body) in [
        (1, "Resetting passwords for locked accounts"),
        (2, "Caf\u{e9} connections and connected devices"),
        (3, "Billing invoice overview"),
    ] {
        run(
            &cassie,
            "INSERT INTO kb (id, body) VALUES ($1, $2)",
            vec![Value::Int64(id), Value::String(body.to_string())],
        );
    }
    run(
        &cassie,
        &format!("CREATE INDEX kb_ft ON kb USING fulltext (body){index_options}"),
        vec![],
    );
    cassie
}

fn matched_ids(cassie: &Cassie, query: &str) -> Vec<Value> {
    run(
        cassie,
        "SELECT id FROM kb WHERE search(body, $1) ORDER BY id",
        vec![Value::String(query.to_string())],
    )
    .into_iter()
    .map(|row| row[0].clone())
    .collect()
}

#[test]
fn should_match_inflected_terms_with_english_stemming() {
    // Arrange
    let cassie = support_fixture(" WITH (stemming = 'english')");

    // Act
    let password = matched_ids(&cassie, "password");
    let connect = matched_ids(&cassie, "connect");

    // Assert
    assert_eq!(password, vec![Value::Int64(1)]);
    assert_eq!(connect, vec![Value::Int64(2)]);
}

#[test]
fn should_not_match_inflected_terms_without_stemming() {
    // Arrange
    let cassie = support_fixture("");

    // Act
    let password = matched_ids(&cassie, "password");

    // Assert
    assert_eq!(password, Vec::<Value>::new());
}

#[test]
fn should_highlight_original_inflected_source_with_english_stemming() {
    // Arrange
    let cassie = support_fixture(" WITH (stemming = 'english', accent_folding = 'true')");

    // Act
    let rows = run(
        &cassie,
        "SELECT snippet(body, 'connect') FROM kb WHERE search(body, 'connect')",
        vec![],
    );

    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0][0],
        Value::String(
            "Caf\u{e9} <mark>connections</mark> and <mark>connected</mark> devices".to_string()
        )
    );
}

#[test]
fn should_expose_stemming_option_in_index_metadata() {
    // Arrange
    let cassie = support_fixture(" WITH (stemming = 'english')");

    // Act
    let rows = run(
        &cassie,
        "SELECT reloptions FROM pg_class WHERE relname = 'kb_ft'",
        vec![],
    );

    // Assert
    assert_eq!(rows.len(), 1);
    let Value::String(definition) = &rows[0][0] else {
        panic!("reloptions should be text");
    };
    assert_eq!(definition, "{stemming=english}");
}

#[test]
fn should_reject_unsupported_stemming_language_at_create_index() {
    // Arrange
    let cassie = support_fixture("");
    let session = cassie.create_session("tester", None);

    // Act
    let result = cassie.execute_sql(
        &session,
        "CREATE INDEX kb_ft2 ON kb USING fulltext (body) WITH (stemming = 'german')",
        vec![],
    );

    // Assert
    assert!(result.is_err());
}

#[test]
fn should_score_stemmed_terms_whose_stem_is_not_idempotent() {
    // Arrange
    let cassie = support_fixture(" WITH (stemming = 'english')");
    run(
        &cassie,
        "INSERT INTO kb (id, body) VALUES (9, 'Television listings')",
        vec![],
    );

    // Act
    let rows = run(
        &cassie,
        "SELECT id, search_score(body, $1) FROM kb WHERE search(body, $1)",
        vec![Value::String("television".to_string())],
    );

    // Assert
    assert_eq!(rows.len(), 1);
    assert!(matches!(rows[0][1], Value::Float64(score) if score > 0.0));
}
