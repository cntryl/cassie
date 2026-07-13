use super::super::projected_read::{is_row_id_column, json_to_query_value};
use super::{
    analyzer_for_search_field, batch, cached_search_context, filter, json_search_term_stats,
    posting_list_candidate_ids, BatchRow, Cassie, CassieSession, FulltextFilteredReadSpec, HashMap,
    Instant, PostingListDocument, QueryError, Value,
};

struct TokenizedFulltextReadDocument {
    id: String,
    payload: serde_json::Value,
    text_stats: filter::SearchTermStats,
}
impl PostingListDocument for TokenizedFulltextReadDocument {
    fn doc_id(&self) -> &str {
        &self.id
    }

    fn term_stats(&self) -> &filter::SearchTermStats {
        &self.text_stats
    }

    fn term_counts(&self) -> &HashMap<String, usize> {
        self.text_stats.term_counts()
    }
}
pub(super) fn execute_fulltext_filtered_read(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &FulltextFilteredReadSpec,
) -> Result<Vec<BatchRow>, QueryError> {
    let started_at = Instant::now();
    let scan_fields = fulltext_filtered_scan_fields(spec);
    let document_batches = cassie
        .scan_projected_documents_batched_for_session(
            session,
            &spec.collection,
            batch::DEFAULT_BATCH_SIZE,
            &scan_fields,
            None,
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
    let search_index_options = super::hybrid::search_context_for_fields(
        cassie,
        &spec.collection,
        std::slice::from_ref(&spec.text_field),
    )?;
    let analyzer = analyzer_for_search_field(&search_index_options, &spec.text_field);
    let search_documents = document_batches
        .into_iter()
        .flat_map(std::iter::IntoIterator::into_iter)
        .map(|document| TokenizedFulltextReadDocument {
            id: document.id,
            text_stats: json_search_term_stats(
                json_projected_value(&document.payload, &spec.text_field),
                &analyzer,
            ),
            payload: document.payload,
        })
        .collect::<Vec<_>>();
    let search_context = cached_search_context(
        cassie,
        &spec.collection,
        &spec.text_field,
        &search_documents,
        super::FulltextSearchTuning {
            boost: &search_index_options.field_boost,
            k1: &search_index_options.field_k1,
            b: &search_index_options.field_b,
            analyzer: &search_index_options.field_analyzer,
        },
    )?;
    let query_terms = filter::prepare_query_terms_with_analyzer(&spec.query, &analyzer);
    let candidate_ids = posting_list_candidate_ids(&search_documents, &query_terms);

    let mut skipped = 0usize;
    let mut rows = Vec::new();
    for document in &search_documents {
        if !candidate_ids.contains(document.id.as_str()) {
            continue;
        }
        let score = search_context.score_term_stats(
            Some(&spec.text_field),
            &document.text_stats,
            &query_terms,
        );
        if score == 0.0 {
            continue;
        }
        if skipped < spec.offset {
            skipped += 1;
            continue;
        }
        if let Some(limit) = spec.limit {
            if rows.len() >= limit {
                break;
            }
        }

        let mut entries = Vec::with_capacity(spec.columns.len().saturating_add(1));
        for column in &spec.columns {
            let value = if is_row_id_column(&column.name) {
                Value::String(document.id.clone())
            } else {
                json_projected_value(&document.payload, &column.name)
                    .map_or(Value::Null, json_to_query_value)
            };
            entries.push((column.output_name.clone(), value));
        }
        entries.push((spec.score_column.clone(), Value::Float64(score)));
        rows.push(BatchRow::new(entries));
    }

    let candidate_count = candidate_ids.len();
    cassie
        .runtime
        .record_search_execution(started_at.elapsed(), candidate_count, rows.len());
    Ok(rows)
}
fn fulltext_filtered_scan_fields(spec: &FulltextFilteredReadSpec) -> Vec<String> {
    let mut fields = vec![spec.text_field.clone()];
    for column in &spec.columns {
        if is_row_id_column(&column.name)
            || fields
                .iter()
                .any(|field| field.eq_ignore_ascii_case(&column.name))
        {
            continue;
        }
        fields.push(column.name.clone());
    }
    fields
}
fn json_projected_value<'a>(
    payload: &'a serde_json::Value,
    field: &str,
) -> Option<&'a serde_json::Value> {
    payload
        .as_object()?
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(field))
        .map(|(_, value)| value)
}
