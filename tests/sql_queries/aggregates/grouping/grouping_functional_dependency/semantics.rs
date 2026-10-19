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
        "SELECT stores.store_key, stores.display_name, COUNT(sales.sale_id), SUM(sales.cents) \
         FROM issue_804_stores AS stores(legacy_id, store_key, display_name) \
         LEFT JOIN issue_804_sales AS sales \
           ON sales.store_id = stores.store_key \
         GROUP BY stores.store_key ORDER BY stores.store_key",
    );
    let unaliased_result = fixture.execute(
        "SELECT issue_804_stores.store_id, issue_804_stores.display_name, \
                COUNT(issue_804_sales.sale_id), SUM(issue_804_sales.cents) \
         FROM issue_804_stores LEFT JOIN issue_804_sales \
           ON issue_804_sales.store_id = issue_804_stores.store_id \
         GROUP BY issue_804_stores.store_id ORDER BY issue_804_stores.store_id",
    );

    // Assert
    let expected = vec![
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
    ];
    assert_eq!(
        result
            .expect("primary-key dependency permits the ungrouped store name")
            .rows(),
        &expected
    );
    assert_eq!(
        unaliased_result
            .expect("unaliased base tables retain their primary-key dependency")
            .rows(),
        &expected
    );
}

#[test]
fn should_select_primary_key_dependent_columns_from_a_single_table() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-primary-key-functional-dependency-single-table",
        &[
            "CREATE TABLE issue_804_keyed_items (item_id INT PRIMARY KEY, display_name TEXT)",
            "INSERT INTO issue_804_keyed_items VALUES (10, 'first'), (20, 'second')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT UPPER(display_name), COUNT(*) FROM issue_804_keyed_items \
         GROUP BY item_id HAVING display_name <> '' ORDER BY display_name",
    );
    let derived_result = fixture.execute(
        "WITH keyed AS (SELECT item_id, display_name FROM issue_804_keyed_items) \
         SELECT display_name, COUNT(*) FROM keyed GROUP BY item_id",
    );

    // Assert
    assert_eq!(
        result
            .expect("a primary key permits dependent columns from a single table")
            .rows(),
        &[
            vec![Value::String("FIRST".to_owned()), Value::Int64(1)],
            vec![Value::String("SECOND".to_owned()), Value::Int64(1)],
        ]
    );
    assert!(derived_result
        .expect_err("a CTE does not inherit the source table primary key")
        .to_string()
        .contains("must appear in the GROUP BY clause"));
}

#[test]
fn should_not_treat_a_unique_key_as_a_primary_key_grouping_dependency() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-unique-key-is-not-primary-key-dependency",
        &[
            "CREATE TABLE issue_804_unique_items (item_code INT UNIQUE, display_name TEXT NOT NULL)",
            "INSERT INTO issue_804_unique_items VALUES (10, 'first'), (20, 'second')",
        ],
    );

    // Act
    let result = fixture
        .execute("SELECT display_name, COUNT(*) FROM issue_804_unique_items GROUP BY item_code");

    // Assert
    assert!(result
        .expect_err("a unique constraint alone does not allow an ungrouped column")
        .to_string()
        .contains("must appear in the GROUP BY clause"));
}
