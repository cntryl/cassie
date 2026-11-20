//! Indexed ranked top-k for `WHERE search(f, q) [AND <residual>] ORDER BY <score> DESC
//! [, _id ASC] LIMIT n [OFFSET m]`.
//!
//! Scores come from persisted postings and corpus-wide persisted statistics, so they equal the
//! row-scan fallback exactly. An eligible scalar equality index is intersected with the posting
//! candidates before any row is fetched. Rows are fetched only for candidates visited in rank
//! order (to apply the residual predicate or project non-identity columns) and fetching stops as
//! soon as `OFFSET + LIMIT` rows qualify. Ties order by score descending, then `_id` ascending,
//! matching the exact path. Any condition that needs the authoritative row scan (transaction
//! overlay, missing index, stale or damaged artifacts, missing candidate row) returns `None` so
//! the caller falls through to the exact path.

use super::fulltext_read::{
    self, document_filter_row, document_filter_row_bytes, fulltext_result_row,
    fulltext_result_row_variable_bytes, PersistedFulltextReadSelection,
};
use super::{
    filter, order_matches_function_score, BatchRow, Cassie, CassieSession, CmpOrdering, Expr,
    FulltextFilteredReadSpec, FunctionMeta, HashMap, Instant, LogicalPlan, QueryError,
    QueryExecutionControls, SelectItem, SortDirection, Value,
};
use crate::runtime::accounted::AccountedVec;

/// A ranked, filtered full-text read that the indexed path can serve.
pub(super) struct RankedFilteredTopKSpec {
    pub(super) read: FulltextFilteredReadSpec,
}

impl RankedFilteredTopKSpec {
    fn limit(&self) -> usize {
        self.read.limit.unwrap_or(0)
    }

    fn top_needed(&self) -> usize {
        self.limit().saturating_add(self.read.offset).max(1)
    }

    /// Whether ranking candidates must be fetched to produce the answer.
    fn needs_row_fetch(&self) -> bool {
        self.read.residual_filter.is_some()
            || !self.read.snippets.is_empty()
            || self
                .read
                .columns
                .iter()
                .any(|column| !crate::types::row_identity::is_row_identity_column(&column.name))
    }
}

pub(super) fn ranked_filtered_top_k_spec(
    plan: &LogicalPlan,
    params: &[Value],
) -> Option<RankedFilteredTopKSpec> {
    let first = plan.order.first()?;
    if plan.order.len() > 2
        || !matches!(first.direction, SortDirection::Desc)
        || first.nulls.is_some()
        || plan.limit_value().is_none()
    {
        return None;
    }
    if let Some(second) = plan.order.get(1) {
        let Expr::Column(column) = &second.expr else {
            return None;
        };
        if !matches!(second.direction, SortDirection::Asc)
            || second.nulls.is_some()
            || !crate::types::row_identity::is_row_identity_column(column)
        {
            return None;
        }
    }
    let SelectItem::Function { function, .. } = plan.projection.last()? else {
        return None;
    };
    let mut unordered = plan.clone();
    unordered.order.clear();
    let read = super::fulltext_filtered_read_spec(&unordered, params)?;
    if !order_matches_function_score(first, function, &read.score_column) {
        return None;
    }
    Some(RankedFilteredTopKSpec { read })
}

fn rank_order(left: &(&String, f64), right: &(&String, f64)) -> CmpOrdering {
    right.1.total_cmp(&left.1).then_with(|| left.0.cmp(right.0))
}

pub(super) fn execute_ranked_filtered_top_k(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    spec: &RankedFilteredTopKSpec,
    controls: &QueryExecutionControls,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    super::super::check_timeout(controls)?;
    if super::virtual_views::schema(&spec.read.collection).is_some()
        || cassie.catalog.get_view(&spec.read.collection).is_some()
    {
        return Ok(None);
    }
    if spec.limit() == 0 {
        return Ok(Some(Vec::new()));
    }
    let started_at = Instant::now();
    let read = &spec.read;
    let persisted = match fulltext_read::load_persisted_fulltext_read(cassie, session, read, controls)?
    {
        PersistedFulltextReadSelection::Ready(persisted) => *persisted,
        PersistedFulltextReadSelection::Exact(_) => return Ok(None),
    };
    let scalar_candidates =
        fulltext_read::scalar_prefilter_ids(cassie, session, read, params, controls)?;
    let matched = fulltext_read::score_persisted_candidates(
        &persisted,
        scalar_candidates.as_ref().map(|candidates| &candidates.ids),
        &read.text_field,
        controls,
    )?;
    let (mut matched, _matched_memory) = matched.into_parts();
    let needs_fetch = spec.needs_row_fetch();
    let needed = spec.top_needed();
    if !needs_fetch && needed < matched.len() {
        matched.select_nth_unstable_by(needed - 1, rank_order);
        matched.truncate(needed);
    }
    matched.sort_by(rank_order);

    let analyzer = persisted
        .search_context
        .analyzer_for_field(&read.text_field);
    let no_payload = serde_json::Value::Null;
    let mut rows = AccountedVec::try_new(controls)?;
    let mut skipped = 0usize;
    let mut row_fetches = 0usize;
    for (id, score) in matched {
        super::super::check_timeout(controls)?;
        if rows.len() >= spec.limit() {
            break;
        }
        let fetched = if needs_fetch {
            let Some(document) = cassie
                .midge
                .get_retrieval_document_controlled(&read.collection, id, controls)
                .map_err(QueryError::from)?
            else {
                return Ok(None);
            };
            row_fetches = row_fetches.saturating_add(1);
            Some(document.into_parts())
        } else {
            None
        };
        let payload = fetched
            .as_ref()
            .map_or(&no_payload, |(document, _memory)| &document.payload);
        if let Some(residual) = &read.residual_filter {
            let _residual_memory =
                controls.reserve_query_memory(document_filter_row_bytes(id, payload))?;
            let candidate_row = document_filter_row(id, payload);
            if filter::filter_rows(
                vec![candidate_row],
                residual,
                params,
                None,
                user_functions,
                session,
            )?
            .is_empty()
            {
                continue;
            }
        }
        if skipped < read.offset {
            skipped += 1;
            continue;
        }
        rows.try_push_with(
            fulltext_result_row_variable_bytes(id, payload, read),
            || {
                fulltext_result_row(
                    id,
                    payload,
                    score,
                    read,
                    (&persisted.query_terms, &analyzer),
                )
            },
        )?;
    }
    let (rows, _output_memory) = rows.into_parts();
    cassie.runtime.record_fulltext_retrieval_diagnostics(
        persisted.candidates.posting_block_reads,
        row_fetches,
    );
    cassie.runtime.record_search_execution(
        started_at.elapsed(),
        persisted.candidates.document_stats.len(),
        rows.len(),
    );
    Ok(Some(rows))
}
