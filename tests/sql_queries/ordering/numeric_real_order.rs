//! ORDER BY, range predicates and grouping on NUMERIC and REAL columns follow the typed
//! value order, with and without a scalar index.

use super::support_sql_fixture::{sql_fixture, SqlFixture};

const NUMERIC_ROWS: &str = "INSERT INTO nt VALUES (1, 2.5), (2, 10), (3, -1.5), \
     (4, 9007199254740993), (5, 9007199254740992.5), (6, 1.0), (7, 1.00)";
const REAL_ROWS: &str = "INSERT INTO rt VALUES (1, 2.5), (2, 10.5), (3, -1.5), (4, 100), (5, 0.25)";

fn numeric_fixture(label: &str, indexed: bool) -> SqlFixture {
    let mut setup = vec![
        "CREATE TABLE nt (id INT PRIMARY KEY, n NUMERIC NOT NULL)",
        NUMERIC_ROWS,
    ];
    if indexed {
        setup.push("CREATE INDEX nt_n ON nt (n)");
    }
    sql_fixture(label, &setup)
}

fn real_fixture(label: &str, indexed: bool) -> SqlFixture {
    let mut setup = vec![
        "CREATE TABLE rt (id INT PRIMARY KEY, r REAL NOT NULL)",
        REAL_ROWS,
    ];
    if indexed {
        setup.push("CREATE INDEX rt_r ON rt (r)");
    }
    sql_fixture(label, &setup)
}

fn ids(fixture: &SqlFixture, sql: &str) -> Vec<i64> {
    fixture
        .rows(sql)
        .iter()
        .map(|row| row[0].as_i64().expect("integer id"))
        .collect()
}

fn sorted_ids(fixture: &SqlFixture, sql: &str) -> Vec<i64> {
    let mut found = ids(fixture, sql);
    found.sort_unstable();
    found
}

#[test]
fn should_order_numeric_by_value_with_limit_without_index() {
    // Arrange
    let fixture = numeric_fixture("numeric_order_limit_plain", false);

    // Act
    let found = ids(&fixture, "SELECT id FROM nt ORDER BY n, id LIMIT 3");

    // Assert
    assert_eq!(found, vec![3, 6, 7]);
}

#[test]
fn should_order_numeric_by_value_descending_with_limit_without_index() {
    // Arrange
    let fixture = numeric_fixture("numeric_order_desc_plain", false);

    // Act
    let found = ids(&fixture, "SELECT id FROM nt ORDER BY n DESC LIMIT 3");

    // Assert
    assert_eq!(found, vec![4, 5, 2]);
}

#[test]
fn should_order_numeric_by_value_without_limit() {
    // Arrange
    let fixture = numeric_fixture("numeric_order_all_plain", false);

    // Act
    let found = ids(&fixture, "SELECT id FROM nt ORDER BY n, id");

    // Assert
    assert_eq!(found, vec![3, 6, 7, 1, 2, 5, 4]);
}

#[test]
fn should_order_real_by_value_with_limit_without_index() {
    // Arrange
    let fixture = real_fixture("real_order_limit_plain", false);

    // Act
    let found = ids(&fixture, "SELECT id FROM rt ORDER BY r LIMIT 3");

    // Assert
    assert_eq!(found, vec![3, 5, 1]);
}

#[test]
fn should_order_real_by_value_descending_with_limit_without_index() {
    // Arrange
    let fixture = real_fixture("real_order_desc_plain", false);

    // Act
    let found = ids(&fixture, "SELECT id FROM rt ORDER BY r DESC LIMIT 2");

    // Assert
    assert_eq!(found, vec![4, 2]);
}

#[test]
fn should_order_double_precision_by_value_with_limit() {
    // Arrange
    let fixture = sql_fixture(
        "double_order_limit_plain",
        &[
            "CREATE TABLE dt (id INT PRIMARY KEY, d DOUBLE PRECISION NOT NULL)",
            "INSERT INTO dt VALUES (1, 2.5), (2, 10), (3, -1.5)",
        ],
    );

    // Act
    let found = ids(&fixture, "SELECT id FROM dt ORDER BY d LIMIT 2");

    // Assert
    assert_eq!(found, vec![3, 1]);
}

#[test]
fn should_filter_numeric_range_through_scalar_index() {
    // Arrange
    let fixture = numeric_fixture("numeric_range_indexed", true);

    // Act
    let found = sorted_ids(&fixture, "SELECT id FROM nt WHERE n > 3");

    // Assert
    assert_eq!(found, vec![2, 4, 5]);
}

#[test]
fn should_filter_numeric_between_through_scalar_index() {
    // Arrange
    let fixture = numeric_fixture("numeric_between_indexed", true);

    // Act
    let found = sorted_ids(&fixture, "SELECT id FROM nt WHERE n BETWEEN 1 AND 10");

    // Assert
    assert_eq!(found, vec![1, 2, 6, 7]);
}

#[test]
fn should_match_numeric_equality_across_scales_through_scalar_index() {
    // Arrange
    let fixture = numeric_fixture("numeric_equality_indexed", true);

    // Act
    let found = sorted_ids(&fixture, "SELECT id FROM nt WHERE n = 1");

    // Assert
    assert_eq!(found, vec![6, 7]);
}

#[test]
fn should_match_numeric_in_list_through_scalar_index() {
    // Arrange
    let fixture = numeric_fixture("numeric_in_indexed", true);

    // Act
    let found = sorted_ids(&fixture, "SELECT id FROM nt WHERE n IN (10, 1.000, -1.5)");

    // Assert
    assert_eq!(found, vec![2, 3, 6, 7]);
}

#[test]
fn should_order_numeric_by_value_through_scalar_index() {
    // Arrange
    let fixture = numeric_fixture("numeric_order_indexed", true);

    // Act
    let ascending = ids(&fixture, "SELECT id FROM nt ORDER BY n, id LIMIT 4");
    let descending = ids(&fixture, "SELECT id FROM nt ORDER BY n DESC LIMIT 2");

    // Assert
    assert_eq!(ascending, vec![3, 6, 7, 1]);
    assert_eq!(descending, vec![4, 5]);
}

#[test]
fn should_distinguish_numeric_values_beyond_double_precision_through_index() {
    // Arrange
    let fixture = numeric_fixture("numeric_wide_indexed", true);

    // Act
    let above = sorted_ids(&fixture, "SELECT id FROM nt WHERE n > 9007199254740992.5");
    let equal = sorted_ids(&fixture, "SELECT id FROM nt WHERE n = 9007199254740993");

    // Assert
    assert_eq!(above, vec![4]);
    assert_eq!(equal, vec![4]);
}

#[test]
fn should_filter_real_range_through_scalar_index() {
    // Arrange
    let fixture = real_fixture("real_range_indexed", true);

    // Act
    let found = sorted_ids(&fixture, "SELECT id FROM rt WHERE r > 3");

    // Assert
    assert_eq!(found, vec![2, 4]);
}

#[test]
fn should_order_real_by_value_through_scalar_index() {
    // Arrange
    let fixture = real_fixture("real_order_indexed", true);

    // Act
    let ascending = ids(&fixture, "SELECT id FROM rt ORDER BY r LIMIT 3");
    let descending = ids(&fixture, "SELECT id FROM rt ORDER BY r DESC LIMIT 2");

    // Assert
    assert_eq!(ascending, vec![3, 5, 1]);
    assert_eq!(descending, vec![4, 2]);
}

#[test]
fn should_match_integral_real_equality_through_scalar_index() {
    // Arrange
    let fixture = real_fixture("real_equality_indexed", true);

    // Act
    let found = sorted_ids(&fixture, "SELECT id FROM rt WHERE r = 100");

    // Assert
    assert_eq!(found, vec![4]);
}

#[test]
fn should_group_numeric_values_by_numeric_equality() {
    // Arrange
    let fixture = numeric_fixture("numeric_group_by", false);

    // Act
    let groups = fixture.rows("SELECT count(*) FROM nt WHERE n = 1 GROUP BY n");
    let distinct = fixture.rows("SELECT DISTINCT n FROM nt WHERE n <= 1.0");

    // Assert
    assert_eq!(groups.len(), 1);
    assert_eq!(distinct.len(), 2);
}

#[test]
fn should_join_numeric_keys_by_numeric_equality() {
    // Arrange
    let fixture = sql_fixture(
        "numeric_join_scales",
        &[
            "CREATE TABLE a (id INT PRIMARY KEY, n NUMERIC)",
            "CREATE TABLE b (id INT PRIMARY KEY, n NUMERIC)",
            "INSERT INTO a VALUES (1, 1.0), (2, 2.50)",
            "INSERT INTO b VALUES (10, 1), (20, 2.5), (30, 3)",
        ],
    );

    // Act
    let found = ids(
        &fixture,
        "SELECT b.id FROM a JOIN b ON a.n = b.n ORDER BY b.id",
    );

    // Assert
    assert_eq!(found, vec![10, 20]);
}

#[test]
fn should_pick_numeric_extremes_by_value() {
    // Arrange
    let fixture = numeric_fixture("numeric_min_max", false);

    // Act
    let rows = fixture.rows("SELECT min(n) = -1.5, max(n) = 9007199254740993 FROM nt");

    // Assert
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][0], cassie::types::Value::Bool(true));
    assert_eq!(rows[0][1], cassie::types::Value::Bool(true));
}

#[test]
fn should_reject_numerically_equal_unique_numeric_values() {
    // Arrange
    let fixture = sql_fixture(
        "numeric_unique",
        &["CREATE TABLE nu (id INT PRIMARY KEY, n NUMERIC UNIQUE)"],
    );
    fixture
        .execute("INSERT INTO nu VALUES (1, 1.0)")
        .expect("first");

    // Act
    let scaled = fixture.execute("INSERT INTO nu VALUES (2, 1.00)");
    let integral = fixture.execute("INSERT INTO nu VALUES (3, 1)");

    // Assert
    assert!(scaled.is_err(), "1.00 must collide with 1.0");
    assert!(integral.is_err(), "1 must collide with 1.0");
}

#[test]
fn should_reject_equal_unique_double_precision_values() {
    // Arrange
    let fixture = sql_fixture(
        "double_unique",
        &["CREATE TABLE du (id INT PRIMARY KEY, d DOUBLE PRECISION UNIQUE)"],
    );
    fixture
        .execute("INSERT INTO du VALUES (1, 1.0)")
        .expect("first");
    fixture
        .execute("INSERT INTO du VALUES (2, 0.0)")
        .expect("zero");

    // Act
    let integral = fixture.execute("INSERT INTO du VALUES (3, 1)");
    let negative_zero = fixture.execute("INSERT INTO du VALUES (4, -0.0)");

    // Assert
    assert!(integral.is_err(), "1 must collide with 1.0");
    assert!(negative_zero.is_err(), "-0.0 must collide with 0.0");
}

#[test]
fn should_reject_equal_unique_real_values() {
    // Arrange
    let fixture = sql_fixture(
        "real_unique",
        &["CREATE TABLE ru (id INT PRIMARY KEY, r REAL UNIQUE)"],
    );
    fixture
        .execute("INSERT INTO ru VALUES (1, 1.5)")
        .expect("first");

    // Act
    let duplicate = fixture.execute("INSERT INTO ru VALUES (2, 1.5)");

    // Assert
    assert!(duplicate.is_err(), "equal REAL values must collide");
}

#[test]
fn should_match_numeric_foreign_key_across_scales() {
    // Arrange
    let fixture = sql_fixture(
        "numeric_fk",
        &[
            "CREATE TABLE np (k NUMERIC PRIMARY KEY)",
            "INSERT INTO np VALUES (1.00)",
        ],
    );
    let created = fixture
        .execute(
            "CREATE TABLE nc (id INT PRIMARY KEY, k NUMERIC, FOREIGN KEY (k) REFERENCES np (k))",
        )
        .err()
        .map(|error| error.to_string());

    // Act
    let inserted = fixture.execute("INSERT INTO nc VALUES (1, 1)");

    // Assert
    assert_eq!(created, None);
    assert!(inserted.is_ok(), "child 1 must match parent 1.00");
}

#[test]
fn should_plan_numeric_range_through_scalar_index() {
    // Arrange
    let fixture = numeric_fixture("numeric_range_plan", true);

    // Act
    let plan = fixture
        .rows("EXPLAIN SELECT id FROM nt WHERE n > 3")
        .iter()
        .map(|row| format!("{:?}", row[0]))
        .collect::<Vec<_>>()
        .join("\n");

    // Assert
    assert!(plan.contains("nt_n"), "range must use the index: {plan}");
}

#[test]
fn should_filter_dotted_numeric_column_range_through_scalar_index() {
    // Arrange
    let fixture = sql_fixture(
        "numeric_dotted_range",
        &[
            "CREATE TABLE dn (id INT PRIMARY KEY, \"N.k\" NUMERIC NOT NULL)",
            "INSERT INTO dn VALUES (1, 2.5), (2, 10), (3, 1.0)",
            "CREATE INDEX dn_k ON dn (\"N.k\")",
        ],
    );

    // Act
    let found = sorted_ids(&fixture, "SELECT id FROM dn WHERE \"N.k\" > 3");

    // Assert
    assert_eq!(found, vec![2]);
}

#[test]
fn should_plan_real_range_through_scalar_index() {
    // Arrange
    let fixture = real_fixture("real_range_plan", true);

    // Act
    let plan = fixture
        .rows("EXPLAIN SELECT id FROM rt WHERE r > 3")
        .iter()
        .map(|row| format!("{:?}", row[0]))
        .collect::<Vec<_>>()
        .join("\n");

    // Assert
    assert!(plan.contains("rt_r"), "range must use the index: {plan}");
}
