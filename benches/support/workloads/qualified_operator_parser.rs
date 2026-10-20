/// One SQL statement with a bounded chain of schema-qualified operators.
pub struct QualifiedOperatorParserFixture {
    statement: String,
}

impl QualifiedOperatorParserFixture {
    #[must_use]
    pub fn new(operator_count: usize) -> Self {
        assert!(
            (1..=2_048).contains(&operator_count),
            "Tier 2 operator chain must contain 1 to 2,048 operators"
        );
        let mut statement = String::from("SELECT 1");
        for _ in 0..operator_count {
            statement.push_str(" OPERATOR(pg_catalog.+) 1");
        }
        Self { statement }
    }

    #[must_use]
    pub fn parse(&self) -> u64 {
        let parsed = cassie::sql::parse_statement(&self.statement)
            .expect("qualified-operator benchmark SQL should parse");
        std::hint::black_box(parsed);
        1
    }
}
