use cassie::app::CassieError;

use crate::support_sql_fixture::{sql_fixture, SqlFixture};

fn width_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE se (a INT, b INT)",
            "CREATE TABLE sf (a INT, b INT)",
            "INSERT INTO sf (a, b) VALUES (1, 2)",
        ],
    )
}

fn is_width_mismatch(error: &CassieError) -> bool {
    error.to_string().contains("column count mismatch")
}

#[test]
fn should_reject_mismatched_set_widths_when_a_branch_is_empty() {
    // Arrange
    let fixture = width_fixture("set_width_empty_branch");
    let mismatched = [
        "SELECT a, b FROM se UNION SELECT a FROM sf",
        "SELECT a FROM se UNION SELECT a, b FROM sf",
        "SELECT a FROM sf INTERSECT SELECT a, b FROM se",
        "SELECT a, b FROM sf EXCEPT SELECT a FROM se",
        "SELECT * FROM se UNION ALL SELECT a FROM sf",
        "WITH w AS (SELECT a, b FROM se) SELECT a FROM sf UNION SELECT * FROM w",
        "SELECT a FROM sf UNION SELECT a FROM sf UNION SELECT a, b FROM se",
    ];

    // Act
    let rejected: Vec<bool> = mismatched
        .iter()
        .map(|sql| {
            fixture
                .execute(sql)
                .err()
                .is_some_and(|e| is_width_mismatch(&e))
        })
        .collect();

    // Assert
    assert_eq!(rejected, vec![true; mismatched.len()]);
}

#[test]
fn should_accept_matching_wildcard_set_widths() {
    // Arrange
    let fixture = width_fixture("set_width_wildcard_match");

    // Act
    let star_left = fixture.rows("SELECT * FROM sf UNION SELECT id, a, b FROM se");
    let star_both = fixture.rows("SELECT * FROM se UNION ALL SELECT * FROM sf");

    // Assert
    let widths: Vec<usize> = star_left.iter().chain(&star_both).map(Vec::len).collect();
    assert_eq!(widths, vec![3, 3]);
}
