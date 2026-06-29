use super::{Cassie, HashSet, FulltextIndexOptions, QueryError, FulltextIndexOptionsCacheKey, HashMap, catalog, AnalyzerConfig};

pub(crate) fn load_fulltext_index_options(
    cassie: &Cassie,
    collection: &str,
    requested_fields: &HashSet<String>,
) -> Result<FulltextIndexOptions, QueryError> {
    let cache_key = FulltextIndexOptionsCacheKey::new(
        cassie.runtime.schema_epoch(),
        collection,
        requested_fields.iter().cloned(),
    );
    if let Some(options) = cassie.runtime.fulltext_index_options_lookup(&cache_key) {
        return Ok(options);
    }

    let mut field_boost = HashMap::new();
    let mut field_k1 = HashMap::new();
    let mut field_b = HashMap::new();
    let mut field_analyzer = HashMap::new();
    let mut seen_fields = HashSet::new();

    for index in cassie.catalog.list_indexes(collection) {
        if index.kind != catalog::IndexKind::FullText {
            continue;
        }

        let field = index.field.to_ascii_lowercase();
        if !requested_fields.contains(&field) {
            continue;
        }
        if !seen_fields.insert(field.clone()) {
            return Err(QueryError::General(format!(
                "fulltext indexes on field '{}' already exist on collection '{}'",
                index.field, collection
            )));
        }

        let boost = parse_index_float_option(
            &index,
            &index.field,
            "boost",
            index.options.get("boost").map(String::as_str),
            crate::search::bm25::DEFAULT_FULLTEXT_BOOST,
            0.0,
            None,
        )?;

        let k1 = parse_index_float_option(
            &index,
            &index.field,
            "k1",
            index.options.get("k1").map(String::as_str),
            crate::search::bm25::DEFAULT_BM25_K1,
            0.0,
            None,
        )?;

        let b = parse_index_float_option(
            &index,
            &index.field,
            "b",
            index.options.get("b").map(String::as_str),
            crate::search::bm25::DEFAULT_BM25_B,
            0.0,
            Some(1.0),
        )?;
        let analyzer = AnalyzerConfig::from_index_options(&index.options)
            .map_err(|error| QueryError::General(error.clone()))?;

        field_boost.insert(field.clone(), boost);
        field_k1.insert(field.clone(), k1);
        field_b.insert(field.clone(), b);
        field_analyzer.insert(field, analyzer);
    }

    let options = FulltextIndexOptions {
        field_boost,
        field_k1,
        field_b,
        field_analyzer,
    };
    cassie
        .runtime
        .store_fulltext_index_options(cache_key, options.clone());
    Ok(options)
}

fn parse_index_float_option(
    index: &catalog::IndexMeta,
    field: &str,
    key: &str,
    value: Option<&str>,
    default: f64,
    min: f64,
    max: Option<f64>,
) -> Result<f64, QueryError> {
    let value = value.unwrap_or("").trim();
    if value.is_empty() {
        return Ok(default);
    }

    let parsed = value.parse::<f64>().map_err(|_| {
        QueryError::General(format!(
            "fulltext index option '{key}' on '{field}' for collection '{}' must be numeric",
            index.collection
        ))
    })?;

    if !parsed.is_finite() {
        return Err(QueryError::General(format!(
            "fulltext index option '{key}' on '{field}' for collection '{}' must be finite",
            index.collection
        )));
    }

    let valid = if let Some(max) = max {
        parsed >= min && parsed <= max
    } else {
        parsed >= min
    };

    if !valid {
        if let Some(max) = max {
            return Err(QueryError::General(format!(
                "fulltext index option '{key}' on '{field}' for collection '{}' must be in [{min}, {max}]",
                index.collection
            )));
        }

        return Err(QueryError::General(format!(
            "fulltext index option '{key}' on '{field}' for collection '{}' must be at least {min}",
            index.collection
        )));
    }

    Ok(parsed)
}
