use super::support_read_equivalence::{sql, with_fixture_config};
use cassie::types::Value;

#[test]
fn should_preserve_relational_phase_order_through_pagination() {
    // Arrange
    for workers in [1, 4] {
        with_fixture_config(
            "typed_relational_phase",
            |config| {
                config.limits.parallel_scan_workers = workers;
                config.limits.parallel_aggregation_workers = workers;
            },
            |cassie, session| {
                sql(
                    cassie,
                    session,
                    "CREATE TABLE phase_rows (category TEXT, score INT, docid INT)",
                );
                sql(cassie, session, "INSERT INTO phase_rows VALUES ('a',9,1),('a',9,2),('a',1,3),('b',5,4),('b',2,5),('c',7,6)");
                let grouped = "SELECT DISTINCT category AS k, COUNT(*) AS total, RANK() OVER (ORDER BY category) AS r FROM phase_rows GROUP BY category HAVING COUNT(*) >= 2 ORDER BY k DESC";
                let winners = "SELECT DISTINCT ON (category) category AS k, score AS s, RANK() OVER (PARTITION BY category ORDER BY score DESC) AS r FROM phase_rows ORDER BY category, score DESC, docid";
                // Act
                let grouped_all = sql(cassie, session, grouped);
                let grouped_page = sql(cassie, session, &format!("{grouped} LIMIT 1 OFFSET 1"));
                let winners_all = sql(cassie, session, winners);
                let winners_page = sql(cassie, session, &format!("{winners} LIMIT 1 OFFSET 1"));
                let zero = sql(cassie, session, &format!("{winners} LIMIT 0 OFFSET 1"));
                // Assert
                assert_eq!(
                    grouped_all.rows(),
                    vec![
                        vec![Value::String("b".into()), Value::Int64(2), Value::Int64(2)],
                        vec![Value::String("a".into()), Value::Int64(3), Value::Int64(1)]
                    ]
                );
                assert_eq!(grouped_page.rows(), &grouped_all.rows()[1..2]);
                assert_eq!(
                    winners_all.rows(),
                    vec![
                        vec![Value::String("a".into()), Value::Int64(9), Value::Int64(1)],
                        vec![Value::String("b".into()), Value::Int64(5), Value::Int64(1)],
                        vec![Value::String("c".into()), Value::Int64(7), Value::Int64(1)]
                    ]
                );
                assert_eq!(winners_page.rows(), &winners_all.rows()[1..2]);
                assert_eq!(zero.rows(), [] as [Vec<Value>; 0]);
                assert_eq!(
                    grouped_page
                        .columns()
                        .iter()
                        .map(|column| column.type_oid)
                        .collect::<Vec<_>>(),
                    grouped_all
                        .columns()
                        .iter()
                        .map(|column| column.type_oid)
                        .collect::<Vec<_>>()
                );
                assert_eq!(
                    winners_page
                        .columns()
                        .iter()
                        .map(|column| column.type_oid)
                        .collect::<Vec<_>>(),
                    winners_all
                        .columns()
                        .iter()
                        .map(|column| column.type_oid)
                        .collect::<Vec<_>>()
                );
            },
        );
    }
}
