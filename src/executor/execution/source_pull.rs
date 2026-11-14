//! Private generated-source cursor tree; ordinary consumers never expand a source first.

use std::mem::size_of;
use std::sync::Arc;

use super::{
    BatchRow, CteContext, Expr, JoinKind, LogicalPlan, QueryError, QuerySource, SourceExecutionEnv,
};
use crate::runtime::QueryMemoryReservation;

enum Cursor<'a> {
    Collection(Box<super::source_collection_pull::Cursor<'a>>),
    Empty,
    Unnest(super::source_set_function::Cursor),
    Series(Box<super::source_series::Cursor>),
    Alias {
        inner: Box<Self>,
        qualifier: String,
        _memory: QueryMemoryReservation,
    },
    Rows(std::vec::IntoIter<BatchRow>),
    Join(Box<JoinCursor<'a>>),
    Query(Box<QueryCursor<'a>>),
}

struct QueryCursor<'a> {
    input: Cursor<'a>,
    plan: LogicalPlan,
    qualifier: String,
    outer: Option<BatchRow>,
    pending: super::projection::OwnedExpandedIter,
    skipped: usize,
    emitted: usize,
    predicate: Option<Predicate>,
    _memory: QueryMemoryReservation,
}

enum Predicate {
    Ready {
        expression: Option<Expr>,
        _memory: QueryMemoryReservation,
    },
    Correlated,
}

struct JoinCursor<'a> {
    left: Cursor<'a>,
    right: Option<Cursor<'a>>,
    current: Option<BatchRow>,
    template: BatchRow,
    kind: JoinKind,
    source: QuerySource,
    matched: bool,
    _memory: QueryMemoryReservation,
}

impl<'a> Cursor<'a> {
    fn open(
        env: &SourceExecutionEnv<'a>,
        source: &QuerySource,
        ctes: &mut CteContext,
        outer: Option<&BatchRow>,
        qualify: bool,
    ) -> Result<Self, QueryError> {
        env.controls.check_interruption()?;
        match source {
            QuerySource::Collection(name) => {
                if let Some(cursor) =
                    super::source_collection_pull::Cursor::open(env, name, qualify)?
                {
                    // Cursor::open already retains size_of::<Self>() in its own lease,
                    // which covers this moved heap allocation for the cursor lifetime.
                    return Ok(Self::Collection(Box::new(cursor)));
                }
                let (batches, _) =
                    super::execute_query_source(env, source, ctes, qualify, outer, None)?;
                Ok(Self::Rows(
                    super::batch::flatten_batches(batches).into_iter(),
                ))
            }
            QuerySource::TableFunction {
                function,
                with_ordinality,
                ..
            } if crate::sql::table_sources::is_series(&function.name) => {
                Ok(Self::Series(Box::new(super::source_series::Cursor::open(
                    env,
                    function,
                    *with_ordinality,
                    outer,
                    ctes,
                )?)))
            }
            QuerySource::TableFunction {
                function,
                with_ordinality,
                ..
            } if crate::sql::table_sources::is_unnest(&function.name) => {
                Ok(Self::Unnest(super::source_set_function::Cursor::open(
                    env,
                    function,
                    *with_ordinality,
                    outer,
                    ctes,
                )?))
            }
            QuerySource::Aliased { source, alias, .. } => {
                let memory = env.controls.reserve_query_memory(size_of::<Self>())?;
                let inner = Self::open(env, source, ctes, outer, false)?;
                let (qualifier, scratch) =
                    super::source_qualification::alias_qualifier(env, alias)?;
                let mut memory = memory;
                memory.try_grow(qualifier.capacity())?;
                drop(scratch);
                Ok(Self::Alias {
                    inner: Box::new(inner),
                    qualifier,
                    _memory: memory,
                })
            }
            QuerySource::Join {
                left, right, kind, ..
            } if matches!(kind, JoinKind::Cross | JoinKind::Inner | JoinKind::Left) => {
                let memory =
                    env.controls
                        .reserve_query_memory(crate::executor::retained_memory::add(
                            size_of::<JoinCursor<'_>>(),
                            crate::sql::parse_analysis::source_clone_bytes(source, env.controls)?,
                        )?)?;
                let left = Self::open(env, left, ctes, outer, true)?;
                let template = super::source_shape::null_row(env, right, ctes)?;
                Ok(Self::Join(Box::new(JoinCursor {
                    left,
                    right: None,
                    current: None,
                    template,
                    kind: *kind,
                    source: source.clone(),
                    matched: false,
                    _memory: memory,
                })))
            }
            QuerySource::Subquery { select, .. } if select.ctes.is_empty() => {
                Self::open_derived(env, source, ctes, outer, qualify)
            }
            _ => {
                let (batches, _) =
                    super::execute_query_source(env, source, ctes, qualify, outer, None)?;
                Ok(Self::Rows(
                    super::batch::flatten_batches(batches).into_iter(),
                ))
            }
        }
    }

    fn open_derived(
        env: &SourceExecutionEnv<'a>,
        source: &QuerySource,
        ctes: &mut CteContext,
        outer: Option<&BatchRow>,
        qualify: bool,
    ) -> Result<Self, QueryError> {
        let QuerySource::Subquery {
            alias,
            select,
            lateral,
        } = source
        else {
            unreachable!("derived source cursor")
        };
        let bytes = crate::sql::parse_analysis::select_clone_bytes(select, env.controls)?;
        let mut memory =
            env.controls
                .reserve_query_memory(crate::executor::retained_memory::add(
                    bytes,
                    size_of::<QueryCursor<'_>>(),
                )?)?;
        let mut plan = super::plan_inspection::logical_plan_from_select(select);
        crate::planner::logical::rewrite_reserved_id_references(&mut plan, &env.cassie.catalog);
        if crate::sql::pagination::plan_needs_resolution(&plan, env.controls)? {
            let (resolved, reservation) =
                crate::sql::pagination::resolve_plan(&plan, env.params, env.controls)?;
            plan = resolved;
            memory.absorb(reservation);
        }
        if plan.limit_value() != Some(0)
            && super::super::exists_plan::plan_has_unresolved_exists(&plan)
        {
            let (resolved, reservation) = super::super::exists_plan::resolve_plan_exists(
                &super::ExistsResolutionContext {
                    cassie: env.cassie,
                    session: env.session,
                    cte_context: ctes,
                    user_functions: env.user_functions,
                    params: env.params,
                    parameter_type_oids: env.parameter_type_oids,
                    controls: env.controls,
                    outer_row: if *lateral { outer } else { None },
                },
                &plan,
            )?;
            plan = resolved;
            memory.absorb(reservation);
        }
        if ordinary(&plan) {
            let outer = if *lateral { outer } else { None };
            if let Some(row) = outer {
                memory.try_grow(crate::executor::retained_memory::add(
                    row.owned_body_bytes()?,
                    row.pending_lookup_bytes()?,
                )?)?;
            }
            memory.try_grow(alias.len())?;
            let input = if plan.limit_value() == Some(0) {
                Self::Empty
            } else {
                Self::open(env, &plan.source, ctes, outer, outer.is_some())?
            };
            return Ok(Self::Query(Box::new(QueryCursor {
                input,
                plan,
                qualifier: alias.clone(),
                outer: outer.cloned(),
                pending: super::projection::OwnedExpanded::untracked(Vec::new()).into_rows(),
                skipped: 0,
                emitted: 0,
                predicate: None,
                _memory: memory,
            })));
        }
        let (batches, _) = super::execute_query_source(env, source, ctes, qualify, outer, None)?;
        Ok(Self::Rows(
            super::batch::flatten_batches(batches).into_iter(),
        ))
    }

    fn next(
        &mut self,
        env: &SourceExecutionEnv<'a>,
        ctes: &mut CteContext,
    ) -> Result<Option<BatchRow>, QueryError> {
        env.controls.check_interruption()?;
        match self {
            Self::Collection(cursor) => cursor.next(env),
            Self::Empty => Ok(None),
            Self::Unnest(cursor) => cursor.next(env),
            Self::Series(cursor) => cursor.next(env),
            Self::Rows(rows) => Ok(rows.next()),
            Self::Alias {
                inner, qualifier, ..
            } => {
                let Some(row) = inner.next(env, ctes)? else {
                    return Ok(None);
                };
                let batches =
                    super::source_qualification::qualify_owned(env, vec![vec![row]], qualifier)?;
                Ok(batches.into_iter().flatten().next())
            }
            Self::Join(join) => join.next(env, ctes),
            Self::Query(query) => query.next(env, ctes),
        }
    }
}

impl<'a> QueryCursor<'a> {
    fn next(
        &mut self,
        env: &SourceExecutionEnv<'a>,
        ctes: &mut CteContext,
    ) -> Result<Option<BatchRow>, QueryError> {
        let limit = usize::try_from(self.plan.limit_value().unwrap_or(i64::MAX).max(0))
            .unwrap_or(usize::MAX);
        let offset =
            usize::try_from(self.plan.offset_value().unwrap_or(0).max(0)).unwrap_or(usize::MAX);
        while self.emitted < limit {
            env.controls.check_interruption()?;
            if let Some(row) = self.pending.next() {
                if self.skipped < offset {
                    self.skipped += 1;
                    continue;
                }
                self.emitted += 1;
                let batches = super::source_qualification::qualify_owned(
                    env,
                    vec![vec![row]],
                    &self.qualifier,
                )?;
                return Ok(batches.into_iter().flatten().next());
            }
            let Some(row) = self.input.next(env, ctes)? else {
                return Ok(None);
            };
            let batches = filtered(
                env,
                &self.plan,
                ctes,
                row,
                self.outer.as_ref(),
                &mut self.predicate,
            )?;
            if batches.iter().all(Vec::is_empty) {
                continue;
            }
            let batches = projected(env, &self.plan, ctes, batches)?;
            self.pending = batches.into_rows();
        }
        Ok(None)
    }
}

fn filtered(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    ctes: &mut CteContext,
    row: BatchRow,
    outer: Option<&BatchRow>,
    state: &mut Option<Predicate>,
) -> Result<Vec<super::Batch>, QueryError> {
    let mut batches = vec![vec![row]];
    if let Some(outer) = outer {
        batches = super::attach_outer_scope(batches, outer, env.controls)?;
    }
    if state.is_none() {
        *state = Some(
            match super::resolve_plan_filter(&env.plan_env(), plan, ctes) {
                Ok(expression) => {
                    let bytes = expression.as_ref().map_or(Ok(0), |expr| {
                        crate::sql::parse_analysis::expression_clone_bytes(expr, 0, env.controls)
                    })?;
                    Predicate::Ready {
                        expression,
                        _memory: env.controls.reserve_query_memory(bytes)?,
                    }
                }
                Err(error) => {
                    if !plan
                        .filter
                        .as_ref()
                        .is_some_and(super::super::exists_correlated::contains_exists)
                    {
                        return Err(error);
                    }
                    Predicate::Correlated
                }
            },
        );
    }
    let (batches, predicate) = match state.as_ref().expect("initialized source predicate") {
        Predicate::Ready { expression, .. } => (batches, expression.as_ref()),
        Predicate::Correlated => {
            let (batches, _) = super::resolve_filter_or_correlate(env, plan, ctes, batches, None)?;
            return Ok(batches);
        }
    };
    super::apply_filter_phase(
        batches,
        predicate,
        env.params,
        None,
        env.user_functions,
        env.session,
        env.controls,
    )
}

fn projected(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    ctes: &mut CteContext,
    batches: Vec<super::Batch>,
) -> Result<super::projection::OwnedExpanded, QueryError> {
    if super::super::exists_projection::contains(&plan.projection) {
        super::exists_projection_phase::apply(env, batches, plan, ctes, None)
            .map(super::projection::OwnedExpanded::untracked)
    } else {
        super::apply_projection_phase(
            batches,
            plan,
            env.params,
            None,
            env.user_functions,
            env.session,
            env.controls,
        )
    }
}

impl<'a> JoinCursor<'a> {
    fn next(
        &mut self,
        env: &SourceExecutionEnv<'a>,
        ctes: &mut CteContext,
    ) -> Result<Option<BatchRow>, QueryError> {
        loop {
            env.controls.check_interruption()?;
            let QuerySource::Join {
                right: right_source,
                on,
                ..
            } = &self.source
            else {
                unreachable!("join cursor specification")
            };
            if self.current.is_none() {
                let Some(left) = self.left.next(env, ctes)? else {
                    return Ok(None);
                };
                if matches!(self.kind, JoinKind::Inner | JoinKind::Left)
                    && !super::source_join::left_terms_pass(
                        env,
                        (&self.source, &*ctes),
                        (&left, None),
                        &self.template,
                        on,
                    )?
                {
                    // A failed preserved-side conjunct matches no right row: a left join
                    // null-extends the row without expanding its right side.
                    if matches!(self.kind, JoinKind::Left) {
                        return combine(env, &left, &self.template).map(Some);
                    }
                    continue;
                }
                self.right = Some(Cursor::open(env, right_source, ctes, Some(&left), true)?);
                self.current = Some(left);
                self.matched = false;
            }
            let left = self.current.as_ref().expect("active left row");
            if let Some(right) = self
                .right
                .as_mut()
                .expect("active right cursor")
                .next(env, ctes)?
            {
                let joined = combine(env, left, &right)?;
                let passes = matches!(self.kind, JoinKind::Cross)
                    || matches!(
                        super::super::exists_phase::evaluate(
                            env,
                            ctes,
                            &self.source,
                            &joined,
                            on,
                            None
                        )?,
                        super::Value::Bool(true)
                    );
                if passes {
                    self.matched = true;
                    return Ok(Some(joined));
                }
                continue;
            }
            let unmatched = matches!(self.kind, JoinKind::Left) && !self.matched;
            let result = unmatched
                .then(|| combine(env, left, &self.template))
                .transpose()?;
            self.current = None;
            self.right = None;
            if result.is_some() {
                return Ok(result);
            }
        }
    }
}

fn combine(
    env: &SourceExecutionEnv<'_>,
    left: &BatchRow,
    right: &BatchRow,
) -> Result<BatchRow, QueryError> {
    let memory = Arc::new(
        env.controls
            .reserve_query_memory(super::source_join::combined_row_bytes(left, right)?)?,
    );
    Ok(super::combine_rows(left, right)?.with_query_memory(Some(memory)))
}

pub(super) fn contains(source: &QuerySource) -> bool {
    match source {
        QuerySource::TableFunction { function, .. } => {
            crate::sql::table_sources::is_generated(&function.name)
        }
        QuerySource::Aliased { source, .. } => contains(source),
        QuerySource::Join { left, right, .. } => contains(left) || contains(right),
        QuerySource::Subquery { select, .. } => contains(&select.source),
        _ => false,
    }
}

fn ordinary(plan: &LogicalPlan) -> bool {
    !super::plan_uses_aggregate(plan)
        && plan.having.is_none()
        && plan.group_by.is_empty()
        && plan.order.is_empty()
        && !plan.distinct
        && plan.distinct_on.is_empty()
        && plan.set.is_none()
        && !plan
            .projection
            .iter()
            .any(|item| matches!(item, crate::sql::ast::SelectItem::WindowFunction { .. }))
        && super::plan_inspection::fulltext_query_fields(plan).is_empty()
}

pub(super) fn try_execute(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
    ctes: &mut CteContext,
    outer: Option<&BatchRow>,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    if !contains(&plan.source) || !ordinary(plan) {
        return Ok(None);
    }
    if plan.limit_value() == Some(0) {
        return Ok(Some(Vec::new()));
    }
    let mut cursor = Cursor::open(env, &plan.source, ctes, outer, outer.is_some())?;
    let offset = usize::try_from(plan.offset_value().unwrap_or(0).max(0)).unwrap_or(usize::MAX);
    let limit =
        usize::try_from(plan.limit_value().unwrap_or(i64::MAX).max(0)).unwrap_or(usize::MAX);
    let cap = limit.min(env.controls.max_result_rows.saturating_add(1));
    let mut skipped = 0;
    let mut output = Vec::new();
    let mut memory = env.controls.reserve_query_memory(0)?;
    let mut predicate = None;
    while output.len() < cap {
        env.controls.check_interruption()?;
        let Some(row) = cursor.next(env, ctes)? else {
            break;
        };
        let batches = filtered(env, plan, ctes, row, outer, &mut predicate)?;
        if batches.iter().all(Vec::is_empty) {
            continue;
        }
        let batches = projected(env, plan, ctes, batches)?;
        for row in batches.into_rows() {
            if skipped < offset {
                skipped += 1;
                continue;
            }
            if output.len() == cap {
                break;
            }
            memory.try_grow(size_of::<BatchRow>())?;
            output.try_reserve_exact(1).map_err(|error| {
                crate::app::CassieError::ResourceLimit(format!(
                    "unable to retain generated result: {error}"
                ))
            })?;
            output.push(row);
        }
    }
    env.controls.check_interruption()?;
    drop(cursor);
    let memory = Arc::new(memory);
    for row in &mut output {
        row.attach_operator_memory(env.controls, Arc::clone(&memory))?;
    }
    Ok(Some(output))
}
