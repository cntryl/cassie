//! PostgreSQL 18.6 GROUP BY primary-key functional dependency contracts for #804.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_select_primary_key_dependent_columns_when_grouping_joined_rows() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-primary-key-functional-dependency",
        &[
            "CREATE TABLE issue_804_stores (store_id INT PRIMARY KEY, display_name TEXT NOT NULL)",
            "CREATE TABLE issue_804_sales (sale_id INT PRIMARY KEY, store_id INT NOT NULL, cents INT NOT NULL)",
            "INSERT INTO issue_804_stores VALUES (1, 'north'), (2, 'south'), (3, 'west')",
            "INSERT INTO issue_804_sales VALUES (10, 1, 125), (11, 1, 175), (12, 2, 250)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT stores.store_id, stores.display_name, COUNT(sales.sale_id), SUM(sales.cents) \
         FROM issue_804_stores AS stores LEFT JOIN issue_804_sales AS sales \
           ON sales.store_id = stores.store_id \
         GROUP BY stores.store_id ORDER BY stores.store_id",
    );

    // Assert
    assert_eq!(
        result
            .expect("primary-key dependency permits the ungrouped store name")
            .rows(),
        &[
            vec![
                Value::Int64(1),
                Value::String("north".to_owned()),
                Value::Int64(2),
                Value::Int64(300),
            ],
            vec![
                Value::Int64(2),
                Value::String("south".to_owned()),
                Value::Int64(1),
                Value::Int64(250),
            ],
            vec![
                Value::Int64(3),
                Value::String("west".to_owned()),
                Value::Int64(0),
                Value::Null,
            ],
        ]
    );
}
