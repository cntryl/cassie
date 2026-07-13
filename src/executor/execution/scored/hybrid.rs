use super::{
    json_search_term_stats_value, value_to_vector, vector_prefilter_supported, BatchRow, Cassie,
    CassieSession, CollectionSchema, FunctionMeta, HybridTopKSpec, QueryError,
    TokenizedHybridDocument, Value,
};
use crate::executor::{batch, filter, scan};
use crate::search::analyzer::AnalyzerConfig;
use std::collections::HashMap;

pub(super) fn search_context_for_fields(
    cassie: &Cassie,
    collection: &str,
    fields: &[String],
) -> Result<super::FulltextIndexOptions, QueryError> {
    let requested_fields = fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    super::load_fulltext_index_options(cassie, collection, &requested_fields)
}

pub(super) fn bounded_hybrid_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: &HybridTopKSpec,
    schema: &CollectionSchema,
    analyzer: &AnalyzerConfig,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    if session.is_some_and(|session| !session.collection_changes(&spec.collection).is_empty()) {
        return Ok(None);
    }
    let Some(fulltext_index) = cassie
        .catalog
        .list_indexes(&spec.collection)
        .into_iter()
        .find(|index| {
            index.kind == crate::catalog::IndexKind::FullText
                && index.field.eq_ignore_ascii_case(&spec.text_field)
        })
    else {
        return Ok(None);
    };
    let vector_ids = match cassie.midge.persisted_vector_candidate_ids(
        &spec.collection,
        &spec.vector_field,
        &spec.vector_query,
        cassie.runtime.limits().adaptive_candidate_max,
    ) {
        Ok(Some(ids)) => ids,
        Ok(None) => return Ok(None),
        Err(_) => {
            cassie
                .runtime
                .record_hybrid_prefilter_usage(0, 0, Some("vector-artifact"));
            return Ok(None);
        }
    };
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, analyzer);
    let Ok(stats) =
        cassie
            .midge
            .fulltext_candidate_stats(&spec.collection, &fulltext_index.name, &query_terms)
    else {
        cassie
            .runtime
            .record_hybrid_prefilter_usage(0, 0, Some("text-artifact"));
        return Ok(None);
    };
    if stats.len() > cassie.runtime.limits().adaptive_candidate_max {
        cassie.runtime.record_hybrid_prefilter_usage(
            stats.len(),
            stats.len(),
            Some("candidate-budget"),
        );
        return Ok(None);
    }
    let fields = vec![spec.text_field.clone(), spec.vector_field.clone()];
    let mut rows = Vec::with_capacity(stats.len().min(vector_ids.len()));
    for id in stats.keys() {
        if !vector_ids.contains(id) {
            continue;
        }
        let Some(document) = cassie
            .get_document_for_session(session, &spec.collection, id)
            .map_err(|error| QueryError::General(error.to_string()))?
        else {
            return Ok(None);
        };
        rows.push(scan::projected_document_to_row(
            document,
            &fields,
            Some(schema),
        ));
    }
    if let Some(filter_expr) = &spec.filter {
        if !vector_prefilter_supported(filter_expr, schema) {
            return Ok(None);
        }
        let before = rows.len();
        rows = filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?;
        cassie
            .runtime
            .record_hybrid_prefilter_usage(before, rows.len(), None);
    }
    Ok(Some(rows))
}

pub(super) fn prefilter_hybrid_rows(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: &HybridTopKSpec,
    schema: &CollectionSchema,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let mut rows = batch::flatten_batches(scan::scan(cassie, session, &spec.collection)?);
    if let Some(filter_expr) = &spec.filter {
        if !vector_prefilter_supported(filter_expr, schema) {
            return Ok(None);
        }
        let before = rows.len();
        rows = filter::filter_rows(rows, filter_expr, params, None, user_functions, session)?;
        cassie
            .runtime
            .record_hybrid_prefilter_usage(before, rows.len(), None);
    }
    Ok(Some(rows))
}

pub(super) fn hybrid_search_documents(
    rows: Vec<BatchRow>,
    spec: &HybridTopKSpec,
    analyzer: &AnalyzerConfig,
) -> Vec<TokenizedHybridDocument> {
    rows.into_iter()
        .map(|row| TokenizedHybridDocument {
            id: row
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            text_stats: json_search_term_stats_value(row.get(&spec.text_field), analyzer),
            vector: row.get(&spec.vector_field).and_then(value_to_vector),
        })
        .collect()
}
