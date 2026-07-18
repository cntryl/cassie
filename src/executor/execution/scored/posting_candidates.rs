use super::{
    filter, HashSet, QueryError, QueryExecutionControls, TokenizedFulltextDocument,
    TokenizedHybridDocument,
};
use crate::runtime::QueryMemoryReservation;

pub(super) trait PostingListDocument {
    fn doc_id(&self) -> &str;
    fn term_stats(&self) -> &filter::SearchTermStats;
    fn term_counts(&self) -> &std::collections::HashMap<String, usize>;
}

impl PostingListDocument for TokenizedFulltextDocument {
    fn doc_id(&self) -> &str {
        &self.id
    }

    fn term_stats(&self) -> &filter::SearchTermStats {
        &self.text_stats
    }

    fn term_counts(&self) -> &std::collections::HashMap<String, usize> {
        self.text_stats.term_counts()
    }
}

impl PostingListDocument for TokenizedHybridDocument {
    fn doc_id(&self) -> &str {
        &self.id
    }

    fn term_stats(&self) -> &filter::SearchTermStats {
        &self.text_stats
    }

    fn term_counts(&self) -> &std::collections::HashMap<String, usize> {
        self.text_stats.term_counts()
    }
}

pub(super) fn posting_list_candidate_ids_controlled<D>(
    documents: &[D],
    query_terms: &[String],
    controls: &QueryExecutionControls,
) -> Result<(HashSet<String>, QueryMemoryReservation), QueryError>
where
    D: PostingListDocument,
{
    let table_bytes = documents.len().saturating_mul(
        std::mem::size_of::<String>().saturating_add(2 * std::mem::size_of::<usize>()),
    );
    let id_bytes = documents
        .iter()
        .map(|document| document.doc_id().len())
        .sum::<usize>();
    let memory = controls.reserve_query_memory(table_bytes.saturating_add(id_bytes))?;
    let mut candidates = HashSet::with_capacity(documents.len());
    for document in documents {
        super::super::check_timeout(controls)?;
        if query_terms
            .iter()
            .any(|term| document.term_counts().contains_key(term))
        {
            candidates.insert(document.doc_id().to_string());
        }
    }
    Ok((candidates, memory))
}
