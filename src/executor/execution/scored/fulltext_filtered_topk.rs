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
    filter, order_matches_function_score, BatchRow, BinaryOp, Cassie, CassieSession, CmpOrdering,
    Expr, FulltextFilteredReadSpec, FunctionMeta, HashMap, Instant, LogicalPlan, QueryError,
    QueryExecutionControls, SelectItem, SortDirection, Value,
};
use crate::runtime::accounted::AccountedVec;

/// Label of this path in the search fallback reasons.
const RANKED_FILTERED_PATH: &str = "ranked_filtered";

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
    let (text_field, query) = super::search_function_args_with_params(function, params)?;
    let mut unordered = plan.clone();
    unordered.order.clear();
    unordered.filter = Some(search_first_filter(
        plan.filter.as_ref()?,
        (&text_field, &query),
        params,
    )?);
    let read = super::fulltext_filtered_read_spec(&unordered, params)?;
    if !order_matches_function_score(first, function, &read.score_column) {
        return None;
    }
    Some(RankedFilteredTopKSpec { read })
}

fn conjuncts<'a>(expr: &'a Expr, out: &mut Vec<&'a Expr>) {
    if let Expr::Binary {
        left,
        op: BinaryOp::And,
        right,
    } = expr
    {
        conjuncts(left, out);
        conjuncts(right, out);
    } else {
        out.push(expr);
    }
}

/// Rewrites `a AND b AND search(..)` (any association) to `search(..) AND (a AND b)` so the
/// shared residual extraction sees one search conjunct and one residual predicate.
fn search_first_filter(
    filter: &Expr,
    search: (&str, &crate::search::query::SearchQueryInput),
    params: &[Value],
) -> Option<Expr> {
    let mut parts = Vec::new();
    conjuncts(filter, &mut parts);
    let position = parts.iter().position(|part| {
        let Expr::Function(function) = part else {
            return false;
        };
        super::search_predicate_args_with_params(function, params).is_some_and(|(field, query)| {
            crate::sql::ColumnIdentifierPath::matches_stored_field(search.0, &field)
                && &query == search.1
        })
    })?;
    let search_part = parts.remove(position).clone();
    let residual = parts
        .into_iter()
        .rev()
        .cloned()
        .reduce(|right, left| Expr::Binary {
            left: Box::new(left),
            op: BinaryOp::And,
            right: Box::new(right),
        });
    Some(match residual {
        None => search_part,
        Some(residual) => Expr::Binary {
            left: Box::new(search_part),
            op: BinaryOp::And,
            right: Box::new(residual),
        },
    })
}

/// Intersects every eligible scalar equality conjunct of the residual predicate.
fn scalar_conjunct_candidates(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    spec: &FulltextFilteredReadSpec,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<Option<fulltext_read::ControlledScalarCandidates>, QueryError> {
    let Some(residual) = spec.residual_filter.as_ref() else {
        return Ok(None);
    };
    let mut parts = Vec::new();
    conjuncts(residual, &mut parts);
    let mut combined: Option<fulltext_read::ControlledScalarCandidates> = None;
    for part in parts {
        let Some(found) = fulltext_read::scalar_equality_candidates(
            cassie,
            session,
            &spec.collection,
            (part, params),
            controls,
        )?
        else {
            continue;
        };
        combined = Some(match combined {
            None => found,
            Some(mut existing) => {
                existing.ids.retain(|id| found.ids.contains(id));
                existing.memory.extend(found.memory);
                existing
            }
        });
    }
    Ok(combined)
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
    let persisted =
        match fulltext_read::load_persisted_fulltext_read(cassie, session, read, controls)? {
            PersistedFulltextReadSelection::Ready(persisted) => *persisted,
            PersistedFulltextReadSelection::Exact(reason) => {
                cassie
                    .runtime
                    .record_fulltext_fallback_cause(RANKED_FILTERED_PATH, reason);
                return Ok(None);
            }
        };
    let scalar_candidates = scalar_conjunct_candidates(cassie, session, read, params, controls)?;
    let matched = fulltext_read::score_persisted_candidates(
        &persisted,
        scalar_candidates.as_ref().map(|candidates| &candidates.ids),
        &read.text_field,
        controls,
    )?;
    let (mut matched, _matched_memory) = matched.into_parts();
    if let Some(scalar_candidates) = &scalar_candidates {
        cassie.runtime.record_fulltext_scalar_prefilter(
            persisted.candidates.document_stats.len(),
            scalar_candidates.ids.len(),
            matched.len(),
        );
    }
    let needs_fetch = spec.needs_row_fetch();
    let needed = spec.top_needed();
    if !needs_fetch && needed < matched.len() {
        matched.select_nth_unstable_by(needed - 1, rank_order);
        matched.truncate(needed);
    }
    matched.sort_by(rank_order);

    let context = PageContext {
        cassie,
        session,
        user_functions,
        params,
        spec,
        persisted: &persisted,
        controls,
    };
    let Some((rows, row_fetches)) = collect_page(&context, matched)? else {
        cassie
            .runtime
            .record_fulltext_fallback_cause(RANKED_FILTERED_PATH, "missing_candidate_row");
        return Ok(None);
    };
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

struct PageContext<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    spec: &'a RankedFilteredTopKSpec,
    persisted: &'a fulltext_read::PersistedFulltextRead,
    controls: &'a QueryExecutionControls,
}

/// Visits ranked candidates, fetching rows only when needed, until the page is full. Returns
/// `None` when a candidate row has vanished so the caller can use the exact path.
fn collect_page(
    context: &PageContext<'_>,
    matched: Vec<(&String, f64)>,
) -> Result<Option<(Vec<BatchRow>, usize)>, QueryError> {
    let PageContext {
        cassie,
        session,
        user_functions,
        params,
        spec,
        persisted,
        controls,
    } = *context;
    let read = &spec.read;
    let needs_fetch = spec.needs_row_fetch();
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
            || fulltext_result_row(id, payload, score, read, (&persisted.search, &analyzer)),
        )?;
    }
    let (rows, _output_memory) = rows.into_parts();
    Ok(Some((rows, row_fetches)))
}
