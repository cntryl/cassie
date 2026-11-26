use super::parse_statement;

const PLANNING_SQL: &str = "SELECT id, title FROM bench_documents WHERE score >= $1 AND status = 'approved' ORDER BY id LIMIT 20";

#[test]
fn should_parse_the_declared_128_statement_fixture() {
    // Arrange
    let statements = vec![PLANNING_SQL; 128];

    // Act
    let parsed = statements
        .iter()
        .map(|sql| parse_statement(sql).expect("benchmark statement should parse"))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(parsed.len(), 128);
}

#[test]
fn should_parse_each_declared_qualified_operator_chain_length() {
    // Arrange
    let chain_lengths = [8, 16, 32, 64, 128];

    // Act
    let parsed = chain_lengths.map(|count| {
        let mut statement = String::from("SELECT 1");
        for _ in 0..count {
            statement.push_str(" OPERATOR(pg_catalog.+) 1");
        }
        parse_statement(&statement).expect("qualified operator statement should parse")
    });

    // Assert
    assert_eq!(parsed.len(), chain_lengths.len());
}
