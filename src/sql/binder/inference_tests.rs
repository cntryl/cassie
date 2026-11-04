use super::infer_select_schema;
use crate::catalog::Catalog;
use crate::sql::QueryStatement;
use crate::types::DataType;

#[test]
fn should_infer_qualified_wildcard_shape_from_unbound_select() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_collection(
        "inference_qualified_star_source",
        vec![
            ("id".to_owned(), DataType::BigInt),
            ("label".to_owned(), DataType::Text),
        ],
    );
    let parsed = crate::sql::parse_statement(
        "SELECT inference_qualified_star_source.* FROM inference_qualified_star_source",
    )
    .expect("qualified wildcard SQL parses");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("qualified wildcard is a SELECT");
    };

    // Act
    let schema = infer_select_schema(&select, &catalog).expect("infer SELECT schema");

    // Assert
    assert_eq!(
        schema
            .fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "label"]
    );
    assert_eq!(schema.fields[0].data_type, DataType::BigInt);
    assert_eq!(schema.fields[1].data_type, DataType::Text);
}
