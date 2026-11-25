//! The analyzer decides the stop-word default until `stop_words` is given explicitly.

use super::fulltext_query_support::run;
use super::support_sql as support;

use cassie::app::Cassie;
use cassie::types::Value;

fn docs(options: &str, rows: &[&str]) -> Cassie {
    let cassie = support::new_in_memory_cassie().expect("create Cassie");
    run(&cassie, "CREATE TABLE docs (label TEXT, body TEXT)", vec![]);
    for (index, body) in rows.iter().enumerate() {
        run(
            &cassie,
            "INSERT INTO docs (label, body) VALUES ($1, $2)",
            vec![
                Value::String(format!("d{index}")),
                Value::String((*body).to_string()),
            ],
        );
    }
    run(
        &cassie,
        &format!("CREATE INDEX docs_body ON docs USING fulltext (body){options}"),
        vec![],
    );
    cassie
}

fn hits(cassie: &Cassie, term: &str) -> usize {
    run(
        cassie,
        "SELECT label FROM docs WHERE search(body, $1)",
        vec![Value::String(term.to_string())],
    )
    .len()
}

#[test]
fn should_match_stop_word_after_altering_analyzer_to_simple() {
    // Arrange
    let cassie = docs("", &["the cat"]);
    let before = hits(&cassie, "the");

    // Act
    run(
        &cassie,
        "ALTER INDEX docs_body SET (analyzer = 'simple')",
        vec![],
    );
    let after = hits(&cassie, "the");

    // Assert
    assert_eq!((before, after), (0, 1));
}

#[test]
fn should_drop_stop_word_after_altering_analyzer_to_standard() {
    // Arrange
    let cassie = docs(" WITH (analyzer = 'simple')", &["the cat"]);
    let before = hits(&cassie, "the");

    // Act
    run(
        &cassie,
        "ALTER INDEX docs_body SET (analyzer = 'standard')",
        vec![],
    );
    let after = hits(&cassie, "the");

    // Assert
    assert_eq!((before, after), (1, 0));
}

#[test]
fn should_keep_explicit_stop_words_given_at_create_when_analyzer_changes() {
    // Arrange
    let cassie = docs(" WITH (stop_words = 'none')", &["the cat"]);

    // Act
    run(
        &cassie,
        "ALTER INDEX docs_body SET (analyzer = 'standard')",
        vec![],
    );

    // Assert
    assert_eq!(hits(&cassie, "the"), 1);
}

#[test]
fn should_apply_explicit_stop_words_given_at_alter() {
    // Arrange
    let cassie = docs(" WITH (analyzer = 'simple')", &["the cat"]);

    // Act
    run(
        &cassie,
        "ALTER INDEX docs_body SET (analyzer = 'simple', stop_words = 'english')",
        vec![],
    );

    // Assert
    assert_eq!(hits(&cassie, "the"), 0);
}

#[test]
fn should_restore_analyzer_default_stop_words_after_reset() {
    // Arrange
    let cassie = docs(
        " WITH (analyzer = 'simple', stop_words = 'english')",
        &["the cat"],
    );
    let before = hits(&cassie, "the");

    // Act
    run(&cassie, "ALTER INDEX docs_body RESET (stop_words)", vec![]);
    let after = hits(&cassie, "the");

    // Assert
    assert_eq!((before, after), (0, 1));
}

#[test]
fn should_filter_stop_words_before_stemming_snippet_tokens() {
    // Arrange
    let cassie = docs(" WITH (stemming = 'english')", &["buts but", "but", "buts"]);

    // Act
    let rows = run(
        &cassie,
        "SELECT search(body, 'buts'), snippet(body, 'buts'), \
         snippet(body, 'buts', 'terms'), snippet(body, 'buts', 'query') \
         FROM docs ORDER BY label",
        vec![],
    );

    // Assert
    assert_eq!(
        rows,
        [
            (true, "<mark>buts</mark> but"),
            (false, "but"),
            (true, "<mark>buts</mark>")
        ]
        .map(|(hit, snippet)| {
            let excerpt = Value::String(snippet.to_string());
            vec![Value::Bool(hit), excerpt.clone(), excerpt.clone(), excerpt]
        })
    );
}
