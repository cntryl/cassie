use super::{
    FulltextFilteredReadSpec, FulltextIndexOptions, QueryError, QueryExecutionControls,
    QueryMemoryReservation, TokenizedFulltextReadDocument,
};
use crate::search::analyzer::AnalyzerConfig;

pub(super) fn reserve_analyzed_text(
    controls: &QueryExecutionControls,
    text: &str,
    analyzer: &AnalyzerConfig,
) -> Result<QueryMemoryReservation, QueryError> {
    controls
        .reserve_query_memory(super::super::memory::tokenized_text_upper_bound(
            text, analyzer,
        ))
        .map_err(QueryError::from)
}

pub(super) fn reserve_search_context(
    controls: &QueryExecutionControls,
    documents: &[TokenizedFulltextReadDocument],
    options: &FulltextIndexOptions,
    field: &str,
) -> Result<QueryMemoryReservation, QueryError> {
    let term_bytes = documents.iter().fold(0usize, |total, document| {
        document
            .text_stats
            .term_counts()
            .keys()
            .fold(total, |bytes, term| {
                bytes.saturating_add(search_map_entry_bytes(term))
            })
    });
    controls
        .reserve_query_memory(
            term_bytes
                .saturating_add(search_option_bytes(options))
                .saturating_add(6 * search_map_entry_bytes(field)),
        )
        .map_err(QueryError::from)
}

pub(super) fn persisted_search_context_bytes(
    candidates: &crate::midge::adapter::fulltext_retrieval::PersistedFulltextCandidateSet,
    options: &FulltextIndexOptions,
    field: &str,
) -> usize {
    candidates
        .document_frequency
        .keys()
        .fold(0usize, |bytes, term| {
            bytes.saturating_add(search_map_entry_bytes(term))
        })
        .saturating_add(search_option_bytes(options))
        .saturating_add(6 * search_map_entry_bytes(field))
}

pub(super) fn document_filter_row_bytes(id: &str, payload: &serde_json::Value) -> usize {
    let payload_entries = payload.as_object().map_or(0, serde_json::Map::len);
    let inline = payload_entries.saturating_add(1).saturating_mul(
        std::mem::size_of::<(String, crate::types::Value)>()
            .saturating_add(2 * std::mem::size_of::<usize>()),
    );
    payload.as_object().map_or(inline, |object| {
        object.iter().fold(
            inline.saturating_add("id".len()).saturating_add(id.len()),
            |bytes, (name, value)| {
                bytes
                    .saturating_add(name.len())
                    .saturating_add(json_retained_bytes(value))
            },
        )
    })
}

pub(super) fn fulltext_result_row_variable_bytes(
    id: &str,
    payload: &serde_json::Value,
    spec: &FulltextFilteredReadSpec,
) -> usize {
    let entry_count = spec
        .columns
        .len()
        .saturating_add(spec.snippets.len())
        .saturating_add(1);
    let inline = entry_count.saturating_mul(
        std::mem::size_of::<(String, crate::types::Value)>()
            .saturating_add(2 * std::mem::size_of::<usize>()),
    );
    let columns = spec.columns.iter().fold(inline, |bytes, column| {
        let value_bytes = if super::is_row_id_column(&column.name) {
            id.len()
        } else {
            super::json_projected_value(payload, &column.name).map_or(0, json_retained_bytes)
        };
        bytes
            .saturating_add(column.output_name.len())
            .saturating_add(value_bytes)
    });
    let snippets = spec.snippets.iter().fold(columns, |bytes, snippet| {
        let source_bytes = super::json_projected_value(payload, &snippet.field)
            .and_then(serde_json::Value::as_str)
            .map_or(0, str::len);
        bytes
            .saturating_add(snippet.output_name.len())
            .saturating_add(source_bytes)
    });
    snippets.saturating_add(spec.score_column.len())
}

fn search_option_bytes(options: &FulltextIndexOptions) -> usize {
    string_key_map_bytes(&options.field_boost)
        .saturating_add(string_key_map_bytes(&options.field_k1))
        .saturating_add(string_key_map_bytes(&options.field_b))
        .saturating_add(options.field_analyzer.keys().fold(0usize, |bytes, field| {
            bytes.saturating_add(search_map_entry_bytes(field))
        }))
}

fn string_key_map_bytes<T>(values: &std::collections::HashMap<String, T>) -> usize {
    values.keys().fold(0usize, |bytes, key| {
        bytes.saturating_add(search_map_entry_bytes(key))
    })
}

fn search_map_entry_bytes(value: &str) -> usize {
    value
        .len()
        .saturating_add(std::mem::size_of::<String>())
        .saturating_add(3 * std::mem::size_of::<usize>())
}

fn json_retained_bytes(value: &serde_json::Value) -> usize {
    let inline = std::mem::size_of::<serde_json::Value>();
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            inline
        }
        serde_json::Value::String(value) => inline.saturating_add(value.len()),
        serde_json::Value::Array(values) => values.iter().fold(inline, |bytes, value| {
            bytes.saturating_add(json_retained_bytes(value))
        }),
        serde_json::Value::Object(values) => values.iter().fold(inline, |bytes, (key, value)| {
            bytes
                .saturating_add(std::mem::size_of::<String>())
                .saturating_add(key.len())
                .saturating_add(json_retained_bytes(value))
        }),
    }
}
