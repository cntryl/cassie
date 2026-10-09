use crate::support_relational_qualification::fixture;
use cassie::types::Value;

#[test]
fn should_order_correlated_exists_by_actual_input() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE phase_outer(id BIGINT)",
        "CREATE TABLE phase_inner(id BIGINT)",
        "INSERT INTO phase_outer VALUES (1),(2)",
        "INSERT INTO phase_inner VALUES (1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("order setup");
    }
    let queries = [
        "SELECT id FROM phase_outer q ORDER BY EXISTS(SELECT 1 FROM phase_inner u WHERE u.id=q.id),id",
        "SELECT id FROM phase_outer q ORDER BY EXISTS(SELECT 1 FROM phase_inner u WHERE u.id=q.id),id LIMIT 1",
        "SELECT id,COUNT(*) FROM phase_outer q GROUP BY id ORDER BY EXISTS(SELECT 1 FROM phase_inner u WHERE u.id=q.id),id",
        "SELECT id FROM phase_outer q ORDER BY CASE WHEN id=1 THEN EXISTS(SELECT 1 FROM phase_inner u WHERE u.id=q.id) ELSE FALSE END,id",
    ];
    // Act
    let results = queries
        .iter()
        .map(|sql| fixture.cassie.execute_sql(&fixture.session, sql, vec![]))
        .collect::<Vec<_>>();
    // Assert
    for ((sql, result), index) in queries.iter().zip(results).zip(0..) {
        let result = result.unwrap_or_else(|error| panic!("{sql}: {error}"));
        let expected = match index {
            1 => vec![vec![Value::Int64(2)]],
            2 => vec![
                vec![Value::Int64(2), Value::Int64(1)],
                vec![Value::Int64(1), Value::Int64(1)],
            ],
            _ => vec![vec![Value::Int64(2)], vec![Value::Int64(1)]],
        };
        assert_eq!(result.rows, expected, "{sql}");
        assert_eq!(
            (
                result.columns[0].type_oid,
                result.columns[0].typlen,
                result.columns[0].atttypmod,
                result.columns[0].format_code
            ),
            (20, 8, -1, 0)
        );
    }
}

#[test]
fn should_choose_distinct_on_representatives_after_correlated_order_keys() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE order_choice(bucket BIGINT,id BIGINT)",
        "CREATE TABLE order_member(id BIGINT)",
        "INSERT INTO order_choice VALUES (1,1),(1,2),(2,3)",
        "INSERT INTO order_member VALUES (1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("distinct order setup");
    }
    let sql="SELECT DISTINCT ON (bucket) id FROM order_choice q ORDER BY bucket,EXISTS(SELECT 1 FROM order_member u WHERE u.id=q.id),id";
    // Act
    let result = fixture.cassie.execute_sql(&fixture.session, sql, vec![]);
    // Assert
    assert_eq!(
        result.expect("actual ordered representative").rows,
        vec![vec![Value::Int64(2)], vec![Value::Int64(3)]]
    );
}

#[test]
fn should_respect_lazy_correlated_order_branch_selection() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE lazy_order(id BIGINT)",
        "CREATE TABLE lazy_member(id BIGINT)",
        "INSERT INTO lazy_order VALUES (1),(2)",
        "INSERT INTO lazy_member VALUES (1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("lazy order setup");
    }
    let dead="SELECT id FROM lazy_order q ORDER BY CASE WHEN id<0 THEN EXISTS(SELECT 1 FROM lazy_member u WHERE u.id=q.id AND 1/0=0) ELSE FALSE END,id";
    let reached="SELECT id FROM lazy_order q ORDER BY EXISTS(SELECT 1 FROM lazy_member u WHERE u.id=q.id AND 1/0=0),id";
    // Act
    let dead_result = fixture.cassie.execute_sql(&fixture.session, dead, vec![]);
    let reached_result = fixture
        .cassie
        .execute_sql(&fixture.session, reached, vec![]);
    // Assert
    assert_eq!(
        dead_result.expect("dead correlated key").rows,
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    assert!(
        reached_result.is_err(),
        "selected correlated ORDER branch must fail"
    );
}

#[test]
fn should_resolve_correlated_order_output_aliases_before_key_evaluation() {
    // Arrange
    let fixture = fixture();
    for sql in [
        "CREATE TABLE alias_order(id BIGINT)",
        "CREATE TABLE alias_member(id BIGINT)",
        "INSERT INTO alias_order VALUES (1),(2)",
        "INSERT INTO alias_member VALUES (1)",
    ] {
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![])
            .expect("alias order setup");
    }
    let sql="SELECT id,EXISTS(SELECT 1 FROM alias_member u WHERE u.id=q.id) AS matched FROM alias_order q ORDER BY matched,id";
    // Act
    let result = fixture.cassie.execute_sql(&fixture.session, sql, vec![]);
    // Assert
    assert_eq!(
        result.expect("alias resolved key").rows,
        vec![
            vec![Value::Int64(2), Value::Bool(false)],
            vec![Value::Int64(1), Value::Bool(true)]
        ]
    );
}
