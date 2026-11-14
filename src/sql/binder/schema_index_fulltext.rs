//! Binding of `CREATE INDEX ... USING fulltext`: the text-field requirement,
//! one fulltext index per field, and the BM25 and analyzer options.

use super::{
    bm25, parse_fulltext_index_float_option, CassieError, Catalog, CollectionSchema,
    CreateIndexStatement, DataType,
};

pub(super) fn bind_fulltext_index_options(
    statement: &mut CreateIndexStatement,
    catalog: &Catalog,
    schema: &CollectionSchema,
    table: &str,
    name: &str,
    fields: &[String],
) -> Result<(), CassieError> {
    let field = &fields[0];
    let field_entry = schema
        .fields
        .iter()
        .find(|entry| entry.name == *field)
        .ok_or_else(|| {
            CassieError::Planner(format!(
                "index field '{field}' does not exist on collection '{table}'"
            ))
        })?;

    if !matches!(field_entry.data_type, DataType::Text) {
        return Err(CassieError::Planner(format!(
            "fulltext index '{name}' requires text field '{field}'"
        )));
    }

    validate_fulltext_uniqueness(catalog, table, name, field)?;
    validate_fulltext_index_option_keys(statement, table, name)?;

    let boost = parse_fulltext_index_float_option(
        "boost",
        statement.options.get("boost").map(String::as_str),
        bm25::DEFAULT_FULLTEXT_BOOST,
        0.0,
        None,
    )?;
    let k1 = parse_fulltext_index_float_option(
        "k1",
        statement.options.get("k1").map(String::as_str),
        bm25::DEFAULT_BM25_K1,
        0.0,
        None,
    )?;
    let b = parse_fulltext_index_float_option(
        "b",
        statement.options.get("b").map(String::as_str),
        bm25::DEFAULT_BM25_B,
        0.0,
        Some(1.0),
    )?;
    let analyzer = crate::search::analyzer::AnalyzerConfig::from_index_options(&statement.options)
        .map_err(CassieError::Planner)?;

    statement
        .options
        .insert("boost".to_string(), boost.to_string());
    statement.options.insert("k1".to_string(), k1.to_string());
    statement.options.insert("b".to_string(), b.to_string());
    statement
        .options
        .insert("analyzer".to_string(), analyzer.name);
    statement
        .options
        .insert("tokenizer".to_string(), analyzer.tokenizer);
    statement.options.insert(
        "case_folding".to_string(),
        analyzer.case_folding.to_string(),
    );
    statement
        .options
        .insert("stop_words".to_string(), analyzer.stop_words);
    statement
        .options
        .insert("stemming".to_string(), analyzer.stemming);
    statement.options.insert(
        "accent_folding".to_string(),
        analyzer.accent_folding.to_string(),
    );
    Ok(())
}

fn validate_fulltext_uniqueness(
    catalog: &Catalog,
    table: &str,
    name: &str,
    field: &str,
) -> Result<(), CassieError> {
    let existing_fulltext_index = catalog.list_indexes(table).into_iter().find(|metadata| {
        metadata.kind == crate::catalog::IndexKind::FullText
            && crate::sql::ColumnIdentifierPath::stored_field_key(&metadata.field)
                == crate::sql::ColumnIdentifierPath::stored_field_key(field)
    });
    if let Some(existing_fulltext_index) = existing_fulltext_index {
        let existing_index = catalog
            .get_index(table, name)
            .filter(|metadata| metadata.kind == crate::catalog::IndexKind::FullText)
            .filter(|metadata| metadata.field == existing_fulltext_index.field);

        if existing_index.is_none() {
            return Err(CassieError::Planner(format!(
                "fulltext index on field '{field}' already exists on collection '{table}'"
            )));
        }
    }
    Ok(())
}

fn validate_fulltext_index_option_keys(
    statement: &CreateIndexStatement,
    table: &str,
    name: &str,
) -> Result<(), CassieError> {
    for key in statement.options.keys() {
        if !matches!(
            key.as_str(),
            "boost"
                | "k1"
                | "b"
                | "analyzer"
                | "case_folding"
                | "stop_words"
                | "stemming"
                | "accent_folding"
                | "tokenizer"
        ) {
            return Err(CassieError::Planner(format!(
                "unsupported fulltext index option '{key}' for '{name}' on collection '{table}'"
            )));
        }
    }
    Ok(())
}
