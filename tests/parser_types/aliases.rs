use cassie::sql::{parse_statement, IdentifierPath, QuerySource, QueryStatement};

#[test]
fn should_preserve_selected_alias_ast_and_unaliased_source_serialization() {
    // Arrange
    let aliased = parse_statement("SELECT \"R\".\"Key\" FROM public.records AS \"R\"(\"Key\")")
        .expect("alias AST");
    let plain = parse_statement("SELECT * FROM public.records").expect("unaliased AST");

    // Act
    let QueryStatement::Select(aliased) = aliased.statement else {
        panic!("SELECT");
    };
    let QueryStatement::Select(plain) = plain.statement else {
        panic!("SELECT");
    };
    let encoded_alias = serde_json::to_value(&aliased.source).expect("serialize alias");
    let restored: QuerySource =
        serde_json::from_value(encoded_alias.clone()).expect("deserialize alias");
    let encoded_plain = serde_json::to_value(&plain.source).expect("serialize plain source");

    // Assert
    assert_eq!(encoded_alias["Aliased"]["alias"], "\"R\"");
    assert_eq!(encoded_alias["Aliased"]["column_aliases"][0], "\"Key\"");
    assert_eq!(restored, aliased.source);
    assert_eq!(
        encoded_plain,
        serde_json::to_value(QuerySource::Collection(
            IdentifierPath::parse("public.records").expect("same original collection variant")
        ))
        .expect("original variant")
    );
}
