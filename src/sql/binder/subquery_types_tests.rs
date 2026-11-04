//! Implicit lateral table functions inherit their left relation's visible types.
use super::*;
use crate::types::DataType;

#[test]
fn should_annotate_a_series_scalar_sublink_given_a_left_values_alias() {
    // Arrange
    let catalog = Catalog::new();
    let context = BindingContext::default();
    let mut parsed = crate::sql::parse_statement(
        "SELECT s.item FROM (VALUES(3))v(n) CROSS JOIN LATERAL generate_series($1,(SELECT v.n))s(item)",
    ).expect("correlated series SQL");

    // Act
    annotate(&mut parsed, &catalog, &context).expect("sublink types");
    let QueryStatement::Select(select) = &parsed.statement else {
        panic!("series SELECT");
    };
    let QuerySource::Join { right, .. } = &select.source else {
        panic!("lateral join");
    };
    let QuerySource::Aliased { source, .. } = right.as_ref() else {
        panic!("series alias");
    };
    let QuerySource::TableFunction { function, .. } = source.as_ref() else {
        panic!("series source");
    };
    let Expr::Subquery(subquery) = &function.args[1] else {
        panic!("series stop sublink");
    };

    // Assert
    assert_eq!(subquery.data_type, Some(DataType::Int));
}
