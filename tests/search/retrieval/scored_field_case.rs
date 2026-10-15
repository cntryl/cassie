use super::support_sql as support;

use cassie::app::{Cassie, CassieSession};
use cassie::types::{Value, Vector};

use support::{data_dir, use_local_storage};

fn seeded(label: &str, statements: &[&str]) -> (Cassie, CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in statements {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }
    (cassie, session, path)
}

fn text_docs(label: &str, indexed: bool) -> (Cassie, CassieSession, String) {
    let mut statements = vec![
            "CREATE TABLE cs (tag TEXT, body TEXT)",
            "INSERT INTO cs (tag, body) VALUES ('d1', 'alpha alpha'), ('d2', 'alpha beta'), ('d3', 'gamma')",
        ];
    if indexed {
        statements.push("CREATE INDEX cs_body_idx ON cs USING fulltext (body)");
    }
    seeded(label, &statements)
}

fn scores(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect(sql)
        .rows()
        .to_vec()
}

#[test]
fn should_score_an_other_case_field_like_the_declared_spelling_on_the_scan_path() {
    // Arrange
    let (cassie, session, path) = text_docs("scored_case_scan", false);

    // Act
    let upper = scores(
        &cassie,
        &session,
        "SELECT tag, search_score(BODY, 'alpha') AS s FROM cs ORDER BY s DESC LIMIT 2",
    );
    let lower = scores(
        &cassie,
        &session,
        "SELECT tag, search_score(body, 'alpha') AS s FROM cs ORDER BY s DESC LIMIT 2",
    );

    // Assert
    assert_eq!(upper, lower);
    assert_ne!(upper[0][1], Value::Float64(0.0));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_score_an_other_case_field_like_the_declared_spelling_on_the_index_path() {
    // Arrange
    let (cassie, session, path) = text_docs("scored_case_index", true);

    // Act
    let upper = scores(
        &cassie,
        &session,
        "SELECT tag, search_score(BODY, 'alpha') AS s FROM cs ORDER BY s DESC LIMIT 2",
    );
    let lower = scores(
        &cassie,
        &session,
        "SELECT tag, search_score(body, 'alpha') AS s FROM cs ORDER BY s DESC LIMIT 2",
    );

    // Assert
    assert_eq!(upper, lower);
    assert_ne!(upper[0][1], Value::Float64(0.0));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_match_an_other_case_field_in_a_search_predicate() {
    // Arrange
    let (cassie, session, path) = text_docs("scored_case_predicate", true);

    // Act
    let matched = scores(
        &cassie,
        &session,
        "SELECT tag FROM cs WHERE search(BODY, 'gamma')",
    );

    // Assert
    assert_eq!(matched, vec![vec![Value::String("d3".to_string())]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_rank_an_other_case_vector_field_like_the_declared_spelling() {
    // Arrange
    let (cassie, session, path) = seeded(
        "scored_case_vector",
        &["CREATE TABLE vc (tag TEXT, embedding VECTOR(3))"],
    );
    for (tag, vector) in [
        ("near", [1.0, 0.0, 0.0]),
        ("far", [-1.0, 0.0, 0.0]),
        ("side", [0.0, 1.0, 0.0]),
    ] {
        cassie
            .execute_sql(
                &session,
                "INSERT INTO vc (tag, embedding) VALUES ($1, $2)",
                vec![
                    Value::String(tag.to_string()),
                    Value::Vector(Vector::new(vector.to_vec())),
                ],
            )
            .expect("insert vector row");
    }

    // Act
    let upper = cassie.execute_sql(
        &session,
        "SELECT tag, vector_distance(EMBEDDING, '[1,0,0]') AS d FROM vc ORDER BY d ASC LIMIT 2",
        vec![],
    );
    let lower = scores(
        &cassie,
        &session,
        "SELECT tag, vector_distance(embedding, '[1,0,0]') AS d FROM vc ORDER BY d ASC LIMIT 2",
    );

    // Assert
    let upper = upper.map(|result| result.rows().to_vec()).ok();
    assert_eq!(upper, Some(lower));
    let _ = std::fs::remove_dir_all(path);
}
