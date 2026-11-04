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

#[test]
fn should_infer_array_projection_types_through_visible_values_aliases() {
    // Arrange
    let catalog = Catalog::new();
    let cases = [
        "SELECT items,s.items FROM (VALUES ('[-2:-1]={1,NULL}'::INT[])) s(items)",
        "SELECT column2,s.column2 FROM (VALUES (ARRAY[1], 'x')) s(column2,column1)",
    ];

    // Act / Assert
    for sql in cases {
        let parsed = crate::sql::parse_statement(sql).expect("array VALUES aliases parse");
        let QueryStatement::Select(select) = parsed.statement else {
            panic!("array projection is a SELECT");
        };
        let schema = infer_select_schema(&select, &catalog).expect("infer visible alias types");
        assert_eq!(schema.fields.len(), 2, "{sql}");
        assert!(
            schema
                .fields
                .iter()
                .all(|field| field.data_type == DataType::Array(Box::new(DataType::Int))),
            "{sql}"
        );
    }
}

#[test]
fn should_infer_native_array_constructor_function_projection_identity() {
    // Arrange
    let catalog = Catalog::new();
    let parsed = crate::sql::parse_statement("SELECT ARRAY[[[[[[1]]]]]]")
        .expect("rank-six array constructor parses");
    let QueryStatement::Select(select) = parsed.statement else {
        panic!("array constructor is a SELECT");
    };

    // Act
    let schema = infer_select_schema(&select, &catalog).expect("infer array constructor type");

    // Assert
    assert_eq!(schema.fields.len(), 1);
    assert_eq!(
        schema.fields[0].data_type,
        DataType::Array(Box::new(DataType::Int))
    );
}
