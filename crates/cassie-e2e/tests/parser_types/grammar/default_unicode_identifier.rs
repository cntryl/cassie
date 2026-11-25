use super::{sql_fixture, Value};

#[test]
fn should_decode_default_unicode_escape_in_quoted_identifier() {
    // Arrange
    let fixture = sql_fixture("issue-796-default-unicode-identifier-escape", &[]);

    // Act
    let created = fixture.execute(r#"CREATE TABLE U&"issue_796_\0069dentifier" (id INTEGER)"#);
    let inserted = fixture.execute("INSERT INTO issue_796_identifier VALUES (73)");
    let selected = fixture.execute("SELECT id FROM issue_796_identifier ORDER BY id");

    // Assert
    created.expect("create table using the default U& identifier escape");
    inserted.expect("resolve the decoded identifier spelling");
    assert_eq!(
        selected
            .expect("lookup by the decoded identifier spelling")
            .rows(),
        &[vec![Value::Int64(73)]]
    );
}
