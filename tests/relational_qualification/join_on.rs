//! Correlated ON predicates run against actual pairs before null extension.
use crate::support_relational_qualification::fixture;
use cassie::types::Value;

fn run_case(sql: &str, expected: &[Vec<Value>]) {
    let fixture = fixture();
    for statement in [
        "CREATE TABLE on_left (id BIGINT)",
        "CREATE TABLE on_right (id BIGINT)",
        "CREATE TABLE on_inner (id BIGINT)",
        "INSERT INTO on_left VALUES (1),(2)",
        "INSERT INTO on_right VALUES (1),(2)",
        "INSERT INTO on_inner VALUES (1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, statement, vec![])
            .expect("ON fixture");
    }
    let result = fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![])
        .expect("actual pair ON predicate");
    assert_eq!(result.rows, expected);
    assert_eq!(
        fixture.cassie.metrics()["joins"]["last_strategy"],
        "nested_loop"
    );
    assert_eq!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_evaluate_correlated_exists_for_each_equality_join_pair() {
    // Arrange
    let sql = "SELECT q.id,r.id FROM on_left q JOIN on_right r ON q.id=r.id AND EXISTS(SELECT 1 FROM on_inner u WHERE u.id=q.id) ORDER BY q.id";
    // Act
    // Assert
    run_case(sql, &[vec![Value::Int64(1), Value::Int64(1)]]);
}

#[test]
fn should_null_extend_left_rows_after_correlated_on_rejects_all_pairs() {
    // Arrange
    let sql = "SELECT q.id,r.id FROM on_left q LEFT JOIN on_right r ON q.id=r.id AND EXISTS(SELECT 1 FROM on_inner u WHERE u.id=q.id) ORDER BY q.id";
    // Act
    // Assert
    run_case(
        sql,
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Null],
        ],
    );
}

#[test]
fn should_resolve_join_on_exists_against_both_pair_aliases() {
    // Arrange
    let sql = "SELECT q.id,r.id FROM on_left q JOIN on_right r ON EXISTS(SELECT 1 FROM on_inner u WHERE u.id=q.id AND u.id=r.id) ORDER BY q.id,r.id";
    // Act
    // Assert
    run_case(sql, &[vec![Value::Int64(1), Value::Int64(1)]]);
}

#[test]
fn should_preserve_ancestor_scope_through_nested_join_on_exists() {
    // Arrange
    let sql = "SELECT q.id,r.id FROM on_left q JOIN on_right r ON q.id=r.id AND EXISTS(SELECT 1 FROM on_inner u WHERE EXISTS(SELECT 1 FROM on_inner v WHERE v.id=q.id AND v.id=r.id)) ORDER BY q.id";
    // Act
    // Assert
    run_case(sql, &[vec![Value::Int64(1), Value::Int64(1)]]);
}

#[test]
fn should_skip_unreached_correlated_exists_in_join_on_case() {
    // Arrange
    let sql = "SELECT q.id,r.id FROM on_left q JOIN on_right r ON q.id=r.id AND CASE WHEN TRUE THEN TRUE ELSE EXISTS(SELECT 1 FROM on_inner u WHERE u.id=q.id AND 1/0=0) END ORDER BY q.id";
    // Act
    // Assert
    run_case(
        sql,
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)],
        ],
    );
}

#[test]
fn should_skip_unreached_correlated_exists_in_join_on_coalesce() {
    // Arrange
    let sql = "SELECT q.id,r.id FROM on_left q JOIN on_right r ON q.id=r.id AND COALESCE(TRUE,EXISTS(SELECT 1 FROM on_inner u WHERE u.id=q.id AND 1/0=0)) ORDER BY q.id";
    // Act
    // Assert
    run_case(
        sql,
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)],
        ],
    );
}

#[test]
fn should_evaluate_correlated_on_after_each_lateral_pair() {
    // Arrange
    let sql = "SELECT q.id,r.id FROM on_left q JOIN LATERAL (SELECT id FROM on_right WHERE id=q.id) r ON EXISTS(SELECT 1 FROM on_inner u WHERE u.id=q.id AND u.id=r.id) ORDER BY q.id";
    // Act
    // Assert
    run_case(sql, &[vec![Value::Int64(1), Value::Int64(1)]]);
}
