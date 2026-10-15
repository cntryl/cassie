use super::support_sql as support;

use cassie::app::{Cassie, CassieSession};
use cassie::executor::QueryResult;
use cassie::types::Value;

use support::{data_dir, use_local_storage};

fn with_scores(label: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    use_local_storage();
    let path = data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        run(&cassie, &session, "CREATE TABLE scores (g TEXT, v BIGINT)");
        run(
            &cassie,
            &session,
            "INSERT INTO scores (g, v) VALUES ('a', 5), ('b', 1), ('c', 9), ('d', 3)",
        );
        test(&cassie, &session);
    });
    let _ = std::fs::remove_dir_all(path);
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> QueryResult {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => result,
        Err(error) => panic!("statement failed: {sql}: {error}"),
    }
}

fn error_text(cassie: &Cassie, session: &CassieSession, sql: &str) -> String {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(_) => format!("statement unexpectedly succeeded: {sql}"),
        Err(error) => error.to_string(),
    }
}

fn row(g: &str, v: i64) -> Vec<Value> {
    vec![Value::String(g.to_string()), Value::Int64(v)]
}

#[test]
fn should_sort_by_select_list_ordinal_in_either_direction() {
    with_scores("order_by_ordinal_direction", |cassie, session| {
        // Arrange
        let ascending = "SELECT g, v FROM scores ORDER BY 2 ASC";
        let descending = "SELECT g, v FROM scores ORDER BY 2 DESC";
        let by_first = "SELECT g, v FROM scores ORDER BY 1 DESC";

        // Act
        let ascending = run(cassie, session, ascending).rows().to_vec();
        let descending = run(cassie, session, descending).rows().to_vec();
        let by_first = run(cassie, session, by_first).rows().to_vec();

        // Assert
        assert_eq!(
            ascending,
            vec![row("b", 1), row("d", 3), row("a", 5), row("c", 9)]
        );
        assert_eq!(
            descending,
            vec![row("c", 9), row("a", 5), row("d", 3), row("b", 1)]
        );
        assert_eq!(
            by_first,
            vec![row("d", 3), row("c", 9), row("b", 1), row("a", 5)]
        );
    });
}

#[test]
fn should_sort_by_ordinal_of_an_aliased_expression_with_limit() {
    with_scores("order_by_ordinal_expression", |cassie, session| {
        // Arrange
        let sql = "SELECT g, v * 2 AS doubled FROM scores ORDER BY 2 DESC LIMIT 2";

        // Act
        let rows = run(cassie, session, sql).rows().to_vec();

        // Assert
        assert_eq!(rows, vec![row("c", 18), row("a", 10)]);
    });
}

#[test]
fn should_sort_grouped_output_by_group_key_ordinal() {
    with_scores("order_by_ordinal_grouped_key", |cassie, session| {
        // Arrange
        run(
            cassie,
            session,
            "INSERT INTO scores (g, v) VALUES ('a', 10), ('b', 20)",
        );
        let sql = "SELECT g, SUM(v) AS s FROM scores GROUP BY g ORDER BY 1 DESC";

        // Act
        let rows = run(cassie, session, sql).rows().to_vec();

        // Assert
        assert_eq!(
            rows,
            vec![row("d", 3), row("c", 9), row("b", 21), row("a", 15)]
        );
    });
}

#[test]
fn should_treat_aggregate_ordinal_like_its_alias() {
    with_scores("order_by_ordinal_grouped_aggregate", |cassie, session| {
        // Arrange
        let by_ordinal = "SELECT g, SUM(v) AS s FROM scores GROUP BY g ORDER BY 2 DESC";
        let by_alias = "SELECT g, SUM(v) AS s FROM scores GROUP BY g ORDER BY s DESC";

        // Act
        let outcomes = [by_ordinal, by_alias].map(|sql| {
            cassie
                .execute_sql(session, sql, vec![])
                .map(|result| result.rows().to_vec())
                .map_err(|error| error.to_string())
        });

        // Assert
        assert_eq!(outcomes[0], outcomes[1]);
    });
}

#[test]
fn should_sort_set_operation_output_by_ordinal() {
    with_scores("order_by_ordinal_union", |cassie, session| {
        // Arrange
        let sql = "SELECT g, v FROM scores WHERE v < 4 UNION ALL SELECT g, v FROM scores WHERE v > 4 ORDER BY 2 DESC";

        // Act
        let rows = run(cassie, session, sql).rows().to_vec();

        // Assert
        assert_eq!(
            rows,
            vec![row("c", 9), row("a", 5), row("d", 3), row("b", 1)]
        );
    });
}

#[test]
fn should_sort_set_operation_output_by_ordinal_of_an_aliased_column() {
    with_scores("order_by_ordinal_union_alias", |cassie, session| {
        // Arrange
        let sql = "SELECT g AS k, v FROM scores WHERE v < 4 UNION ALL SELECT g, v FROM scores WHERE v > 4 ORDER BY 1 DESC";

        // Act
        let rows = run(cassie, session, sql).rows().to_vec();

        // Assert
        assert_eq!(
            rows,
            vec![row("d", 3), row("c", 9), row("b", 1), row("a", 5)]
        );
    });
}

#[test]
fn should_reject_ordinal_whose_output_name_is_another_items_alias() {
    with_scores("order_by_ordinal_ambiguous", |cassie, session| {
        // Arrange
        let sql = "SELECT g, v AS g FROM scores ORDER BY 1 DESC";

        // Act
        let error = error_text(cassie, session, sql);

        // Assert
        assert!(
            error.contains("ORDER BY position 1 is ambiguous"),
            "unexpected error: {error}"
        );
    });
}

#[test]
fn should_reject_ordinals_outside_the_select_list() {
    with_scores("order_by_ordinal_out_of_range", |cassie, session| {
        // Arrange
        let statements = [
            ("SELECT g, v FROM scores ORDER BY 3", "3"),
            ("SELECT g, v FROM scores ORDER BY 0", "0"),
        ];

        // Act
        let errors = statements.map(|(sql, position)| (error_text(cassie, session, sql), position));

        // Assert
        for (error, position) in errors {
            assert!(
                error.contains(&format!(
                    "ORDER BY position {position} is not in select list"
                )),
                "unexpected error: {error}"
            );
        }
    });
}
