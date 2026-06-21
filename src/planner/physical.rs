use crate::catalog::{CollectionCardinalityStats, IndexKind, IndexMeta};
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{
    BinaryOp, Expr, FunctionCall, JoinKind, QuerySource, SelectItem, WindowFunctionCall,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operator {
    Scan,
    Filter,
    Project,
    Sort,
    Limit,
    Offset,
    VectorSearch,
    FullTextSearch,
    Join,
    Aggregate,
    Distinct,
    SetOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalPlan {
    pub collection: String,
    pub operators: Vec<Operator>,
    pub logical: LogicalPlan,
    pub estimates: PlanEstimates,
    pub predicate_pushdown: bool,
    pub projected_scan_fields: Vec<String>,
    pub scan_limit: Option<usize>,
    pub selected_index: Option<String>,
    pub covered_index: bool,
    pub top_k: bool,
    pub top_k_limit: Option<usize>,
    pub join_strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanEstimates {
    pub scan_rows: u64,
    pub index_rows: u64,
    pub join_rows: u64,
    pub search_rows: u64,
    pub vector_rows: u64,
    pub aggregate_rows: u64,
}

pub fn build(plan: LogicalPlan) -> PhysicalPlan {
    build_with_indexes(plan, Vec::new(), &Default::default())
}

pub fn build_with_indexes(
    plan: LogicalPlan,
    indexes: Vec<IndexMeta>,
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> PhysicalPlan {
    if plan.command.is_some() {
        return PhysicalPlan {
            collection: plan.collection.clone(),
            operators: Vec::new(),
            logical: plan,
            estimates: PlanEstimates::default(),
            predicate_pushdown: false,
            projected_scan_fields: Vec::new(),
            scan_limit: None,
            selected_index: None,
            covered_index: false,
            top_k: false,
            top_k_limit: None,
            join_strategy: None,
        };
    }

    let predicate_pushdown = plan_supports_predicate_pushdown(&plan);
    let projected_scan_fields = projected_scan_fields(&plan).unwrap_or_default();
    let scan_limit = scan_limit(&plan, &projected_scan_fields);
    let selected_index = selected_index(&plan, indexes.as_slice());
    let covered_index = selected_index
        .as_deref()
        .and_then(|name| indexes.iter().find(|index| index.name == name))
        .is_some_and(|index| plan_is_covered_by_index(&plan, index));
    let top_k_limit = top_k_limit(&plan);
    let top_k = top_k_limit.is_some();
    let join_strategy = join_strategy(&plan);
    let estimates = PlanEstimates::from_plan(&plan, selected_index.as_deref(), cardinality_stats);
    let mut operators = vec![Operator::Scan];
    if source_contains_join(&plan.source) {
        operators.push(Operator::Join);
    }
    if plan_uses_fulltext(&plan) {
        operators.push(Operator::FullTextSearch);
    }
    if plan_uses_vector(&plan) {
        operators.push(Operator::VectorSearch);
    }
    if plan.filter.is_some() {
        operators.push(Operator::Filter);
    }
    if plan_uses_aggregate(&plan) {
        operators.push(Operator::Aggregate);
    }
    if plan.distinct || !plan.distinct_on.is_empty() {
        operators.push(Operator::Distinct);
    }
    if plan.set.is_some() {
        operators.push(Operator::SetOperation);
    }
    if !plan.order.is_empty() {
        operators.push(Operator::Sort);
    }
    if !plan.projection.is_empty() {
        operators.push(Operator::Project);
    }
    if plan.offset.is_some() {
        operators.push(Operator::Offset);
    }
    if plan.limit.is_some() {
        operators.push(Operator::Limit);
    }
    PhysicalPlan {
        collection: plan.collection.clone(),
        operators,
        logical: plan,
        estimates,
        predicate_pushdown,
        projected_scan_fields,
        scan_limit,
        selected_index,
        covered_index,
        top_k,
        top_k_limit,
        join_strategy,
    }
}

impl PlanEstimates {
    const DEFAULT_ROWS: u64 = 1_000;

    fn from_plan(
        plan: &LogicalPlan,
        selected_index: Option<&str>,
        cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
    ) -> Self {
        let scan_rows = estimated_source_rows(&plan.source, cardinality_stats);
        let index_rows = selected_index
            .and_then(|name| {
                collection_stats(&plan.collection, cardinality_stats).and_then(|stats| {
                    stats
                        .index_cardinality(&CollectionCardinalityStats::scalar_index_key(name))
                        .or_else(|| {
                            stats.index_cardinality(
                                &CollectionCardinalityStats::fulltext_index_key(name),
                            )
                        })
                        .or_else(|| {
                            stats.index_cardinality(&CollectionCardinalityStats::vector_index_key(
                                name,
                            ))
                        })
                        .or_else(|| {
                            stats.index_cardinality(&CollectionCardinalityStats::hybrid_index_key(
                                name,
                            ))
                        })
                })
            })
            .unwrap_or(scan_rows);
        let join_rows = join_rows(&plan.source, cardinality_stats);
        let search_rows = if plan_uses_fulltext(plan) {
            scan_rows.saturating_div(2).max(1)
        } else {
            0
        };
        let vector_rows = if plan_uses_vector(plan) {
            scan_rows.saturating_div(2).max(1)
        } else {
            0
        };
        let aggregate_rows = if plan_uses_aggregate(plan) {
            if plan.group_by.is_empty() {
                1
            } else {
                scan_rows.saturating_div(2).max(1)
            }
        } else {
            0
        };

        Self {
            scan_rows,
            index_rows,
            join_rows,
            search_rows,
            vector_rows,
            aggregate_rows,
        }
    }
}

fn collection_stats<'a>(
    collection: &str,
    cardinality_stats: &'a std::collections::HashMap<String, CollectionCardinalityStats>,
) -> Option<&'a CollectionCardinalityStats> {
    cardinality_stats
        .get(collection)
        .filter(|stats| stats.hydrated)
}

fn estimated_source_rows(
    source: &QuerySource,
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> u64 {
    match source {
        QuerySource::Collection(collection) => collection_stats(collection, cardinality_stats)
            .map(|stats| stats.row_count)
            .unwrap_or(PlanEstimates::DEFAULT_ROWS),
        QuerySource::Join {
            left,
            right,
            kind,
            on,
        } => {
            let left_rows = estimated_source_rows(left, cardinality_stats);
            let right_rows = estimated_source_rows(right, cardinality_stats);
            match kind {
                JoinKind::Inner if is_equi_join_predicate(on) => left_rows.min(right_rows),
                JoinKind::Cross => left_rows.saturating_mul(right_rows),
                _ => left_rows.saturating_add(right_rows),
            }
        }
        QuerySource::Subquery { select, .. } => select
            .limit
            .and_then(|limit| usize::try_from(limit.max(0)).ok())
            .map(|limit| limit as u64)
            .unwrap_or(PlanEstimates::DEFAULT_ROWS),
        QuerySource::Cte(_) => PlanEstimates::DEFAULT_ROWS,
        QuerySource::SingleRow => 1,
    }
}

fn join_rows(
    source: &QuerySource,
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> u64 {
    match source {
        QuerySource::Join {
            left,
            right,
            kind,
            on,
        } => {
            let left_rows = estimated_source_rows(left, cardinality_stats);
            let right_rows = estimated_source_rows(right, cardinality_stats);
            match kind {
                JoinKind::Inner if is_equi_join_predicate(on) => left_rows.min(right_rows),
                JoinKind::Cross => left_rows.saturating_mul(right_rows),
                _ => left_rows.saturating_add(right_rows),
            }
        }
        _ => 0,
    }
}

fn join_strategy(plan: &LogicalPlan) -> Option<String> {
    match &plan.source {
        QuerySource::Join {
            kind: JoinKind::Inner,
            on,
            ..
        } if is_equi_join_predicate(on) => Some("hash".to_string()),
        QuerySource::Join { .. } => Some("nested_loop".to_string()),
        _ if plan.filter.as_ref().is_some_and(expr_contains_not_exists) => Some("anti".to_string()),
        _ if plan.filter.as_ref().is_some_and(expr_contains_exists) => Some("semi".to_string()),
        _ => None,
    }
}

fn expr_contains_exists(expr: &Expr) -> bool {
    match expr {
        Expr::Exists(_) => true,
        Expr::Binary { left, right, .. } => {
            expr_contains_exists(left) || expr_contains_exists(right)
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } => expr_contains_exists(expr),
        Expr::InList { expr, values, .. } => {
            expr_contains_exists(expr) || values.iter().any(expr_contains_exists)
        }
        Expr::Between {
            expr, low, high, ..
        } => expr_contains_exists(expr) || expr_contains_exists(low) || expr_contains_exists(high),
        Expr::Not { .. }
        | Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Function(_) => false,
    }
}

fn expr_contains_not_exists(expr: &Expr) -> bool {
    expr_contains_not_exists_with_polarity(expr, false)
}

fn expr_contains_not_exists_with_polarity(expr: &Expr, negated: bool) -> bool {
    match expr {
        Expr::Not { expr } => expr_contains_not_exists_with_polarity(expr, !negated),
        Expr::Exists(_) => negated,
        Expr::Binary { left, right, .. } => {
            expr_contains_not_exists_with_polarity(left, negated)
                || expr_contains_not_exists_with_polarity(right, negated)
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } => {
            expr_contains_not_exists_with_polarity(expr, negated)
        }
        Expr::InList { expr, values, .. } => {
            expr_contains_not_exists_with_polarity(expr, negated)
                || values
                    .iter()
                    .any(|value| expr_contains_not_exists_with_polarity(value, negated))
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            expr_contains_not_exists_with_polarity(expr, negated)
                || expr_contains_not_exists_with_polarity(low, negated)
                || expr_contains_not_exists_with_polarity(high, negated)
        }
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Function(_) => false,
    }
}

fn is_equi_join_predicate(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right
        } if matches!((left.as_ref(), right.as_ref()), (Expr::Column(_), Expr::Column(_)))
    )
}

fn top_k_limit(plan: &LogicalPlan) -> Option<usize> {
    if plan.order.is_empty() || plan.limit.is_none() {
        return None;
    }
    let limit = usize::try_from(plan.limit?.max(0)).ok()?;
    let offset = usize::try_from(plan.offset.unwrap_or(0).max(0)).ok()?;
    limit.checked_add(offset)
}

fn selected_index(plan: &LogicalPlan, indexes: &[IndexMeta]) -> Option<String> {
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let filter = plan.filter.as_ref()?;
    let equality_fields = equality_filter_fields(filter);
    indexes
        .iter()
        .filter(|index| index.collection == *collection && index.kind == IndexKind::Scalar)
        .filter(|index| partial_index_matches_query(plan.filter.as_ref(), index.predicate.as_ref()))
        .filter(|index| {
            index
                .normalized_fields()
                .iter()
                .all(|field| equality_fields.contains(&field.to_ascii_lowercase()))
        })
        .max_by_key(|index| index.normalized_fields().len())
        .map(|index| index.name.clone())
}

fn partial_index_matches_query(
    query_filter: Option<&Expr>,
    index_predicate: Option<&String>,
) -> bool {
    match index_predicate {
        None => true,
        Some(predicate) => {
            query_filter
                .and_then(|filter| serde_json::to_string(filter).ok())
                .as_ref()
                == Some(predicate)
        }
    }
}

fn plan_is_covered_by_index(plan: &LogicalPlan, index: &IndexMeta) -> bool {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || source_contains_join(&plan.source)
    {
        return false;
    }

    if plan
        .filter
        .as_ref()
        .is_some_and(|filter| !filter_supports_covering_index(filter))
    {
        return false;
    }

    let covered_fields = index
        .normalized_fields()
        .into_iter()
        .chain(index.normalized_include_fields())
        .map(|field| field.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let mut needed_fields = BTreeSet::new();

    for item in &plan.projection {
        match item {
            SelectItem::Column { name, .. } if is_row_id_column(name) => {}
            SelectItem::Column { name, .. } => {
                needed_fields.insert(name.to_ascii_lowercase());
            }
            _ => return false,
        }
    }
    for expr in plan_expressions(plan) {
        collect_expr_column_refs(expr, &mut needed_fields);
    }

    needed_fields
        .into_iter()
        .all(|field| covered_fields.contains(&field))
}

fn filter_supports_covering_index(expr: &Expr) -> bool {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => filter_supports_covering_index(left) && filter_supports_covering_index(right),
        Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } => matches!(
            (left.as_ref(), right.as_ref()),
            (Expr::Column(_), Expr::StringLiteral(_))
                | (Expr::StringLiteral(_), Expr::Column(_))
                | (Expr::Column(_), Expr::NumberLiteral(_))
                | (Expr::NumberLiteral(_), Expr::Column(_))
                | (Expr::Column(_), Expr::BoolLiteral(_))
                | (Expr::BoolLiteral(_), Expr::Column(_))
                | (Expr::Column(_), Expr::Null)
                | (Expr::Null, Expr::Column(_))
                | (Expr::Column(_), Expr::Param(_))
                | (Expr::Param(_), Expr::Column(_))
        ),
        _ => false,
    }
}

fn collect_expr_column_refs(expr: &Expr, fields: &mut BTreeSet<String>) {
    match expr {
        Expr::Column(name) if is_row_id_column(name) => {}
        Expr::Column(name) => {
            fields.insert(name.to_ascii_lowercase());
        }
        Expr::Binary { left, right, .. } => {
            collect_expr_column_refs(left, fields);
            collect_expr_column_refs(right, fields);
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            collect_expr_column_refs(expr, fields);
        }
        Expr::InList { expr, values, .. } => {
            collect_expr_column_refs(expr, fields);
            for value in values {
                collect_expr_column_refs(value, fields);
            }
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_expr_column_refs(expr, fields);
            collect_expr_column_refs(low, fields);
            collect_expr_column_refs(high, fields);
        }
        Expr::Function(function) => {
            for arg in &function.args {
                collect_expr_column_refs(arg, fields);
            }
        }
        Expr::Exists(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Param(_) => {}
    }
}

fn equality_filter_fields(expr: &Expr) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    collect_equality_filter_fields(expr, &mut fields);
    fields
}

fn collect_equality_filter_fields(expr: &Expr, fields: &mut BTreeSet<String>) {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_equality_filter_fields(left, fields);
            collect_equality_filter_fields(right, fields);
        }
        Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } => match (left.as_ref(), right.as_ref()) {
            (Expr::Column(field), value) | (value, Expr::Column(field))
                if !matches!(value, Expr::Column(_)) =>
            {
                fields.insert(field.to_ascii_lowercase());
            }
            _ => {}
        },
        _ => {}
    }
}

fn scan_limit(plan: &LogicalPlan, projected_scan_fields: &[String]) -> Option<usize> {
    if projected_scan_fields.is_empty() || plan.filter.is_some() {
        return None;
    }
    let limit = plan.limit?;
    let limit = usize::try_from(limit.max(0)).ok()?;
    let offset = usize::try_from(plan.offset.unwrap_or(0).max(0)).ok()?;
    limit.checked_add(offset)
}

fn plan_supports_predicate_pushdown(plan: &LogicalPlan) -> bool {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !plan.order.is_empty()
    {
        return false;
    }

    if !matches!(plan.source, QuerySource::Collection(_)) {
        return false;
    }

    if plan.projection.is_empty()
        || !plan
            .projection
            .iter()
            .all(|item| matches!(item, SelectItem::Column { .. }))
    {
        return false;
    }

    plan.filter
        .as_ref()
        .is_some_and(filter_supports_predicate_pushdown)
}

fn projected_scan_fields(plan: &LogicalPlan) -> Option<Vec<String>> {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !plan.order.is_empty()
    {
        return None;
    }

    if !matches!(plan.source, QuerySource::Collection(_)) {
        return None;
    }

    let projection_columns = plan
        .projection
        .iter()
        .map(|item| match item {
            SelectItem::Column { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if projection_columns.is_empty() {
        return None;
    }

    let filter_columns = match plan.filter.as_ref() {
        Some(filter) => projected_filter_columns(filter)?,
        None => Vec::new(),
    };

    let mut fields = Vec::new();
    for column in projection_columns.into_iter().chain(filter_columns) {
        if is_row_id_column(&column) || fields.iter().any(|field: &String| field == &column) {
            continue;
        }
        fields.push(column);
    }
    Some(fields)
}

fn projected_filter_columns(expr: &Expr) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    collect_projected_filter_columns(expr, &mut fields)?;
    Some(fields)
}

fn collect_projected_filter_columns(expr: &Expr, fields: &mut Vec<String>) -> Option<()> {
    match expr {
        Expr::Column(name) => {
            if !fields.iter().any(|field| field.eq_ignore_ascii_case(name)) {
                fields.push(name.clone());
            }
            Some(())
        }
        Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => Some(()),
        Expr::Binary { left, op, right } => {
            match op {
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Lte
                | BinaryOp::Gt
                | BinaryOp::Gte
                | BinaryOp::And
                | BinaryOp::Or
                | BinaryOp::Like => {}
                _ => return None,
            }
            collect_projected_filter_columns(left, fields)?;
            collect_projected_filter_columns(right, fields)
        }
        Expr::IsNull { expr, .. } => collect_projected_filter_columns(expr, fields),
        Expr::InList { expr, values, .. } => {
            collect_projected_filter_columns(expr, fields)?;
            for value in values {
                collect_projected_filter_columns(value, fields)?;
            }
            Some(())
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            collect_projected_filter_columns(expr, fields)?;
            collect_projected_filter_columns(low, fields)?;
            collect_projected_filter_columns(high, fields)
        }
        Expr::Not { expr } | Expr::Cast { expr, .. } => {
            collect_projected_filter_columns(expr, fields)
        }
        Expr::Function(_) | Expr::Exists(_) => None,
    }
}

fn filter_supports_predicate_pushdown(expr: &Expr) -> bool {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = expr
    else {
        return false;
    };

    match (left.as_ref(), right.as_ref()) {
        (Expr::Column(field), literal) | (literal, Expr::Column(field)) => {
            !is_row_id_column(field) && expr_is_pushdown_literal(literal)
        }
        _ => false,
    }
}

fn expr_is_pushdown_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::StringLiteral(_) | Expr::BoolLiteral(_) | Expr::Null
    )
}

fn is_row_id_column(field: &str) -> bool {
    field == "_id" || field.eq_ignore_ascii_case("id")
}

fn source_contains_join(source: &QuerySource) -> bool {
    match source {
        QuerySource::Join { .. } => true,
        QuerySource::Subquery { select, .. } => source_contains_join(&select.source),
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
    }
}

fn plan_uses_aggregate(plan: &LogicalPlan) -> bool {
    !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.projection.iter().any(|item| match item {
            SelectItem::Function { function, .. } => {
                crate::sql::functions::is_aggregate_function(&function.name)
            }
            SelectItem::Wildcard
            | SelectItem::Column { .. }
            | SelectItem::Expr { .. }
            | SelectItem::WindowFunction { .. } => false,
        })
}

fn plan_uses_fulltext(plan: &LogicalPlan) -> bool {
    plan.projection.iter().any(select_item_uses_fulltext)
        || plan_expressions(plan).any(expr_uses_fulltext)
}

fn plan_uses_vector(plan: &LogicalPlan) -> bool {
    plan.projection.iter().any(select_item_uses_vector)
        || plan_expressions(plan).any(expr_uses_vector)
}

fn plan_expressions(plan: &LogicalPlan) -> impl Iterator<Item = &Expr> {
    plan.projection
        .iter()
        .flat_map(select_item_expressions)
        .chain(plan.filter.iter())
        .chain(plan.group_by.iter())
        .chain(plan.having.iter())
        .chain(plan.order.iter().map(|order| &order.expr))
        .chain(plan.distinct_on.iter())
}

fn select_item_expressions(item: &SelectItem) -> Vec<&Expr> {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => Vec::new(),
        SelectItem::Function { function, .. } => function.args.iter().collect(),
        SelectItem::Expr { expr, .. } => vec![expr],
        SelectItem::WindowFunction { function, .. } => window_function_expressions(function),
    }
}

fn window_function_expressions(function: &WindowFunctionCall) -> Vec<&Expr> {
    function
        .args
        .iter()
        .chain(function.partition_by.iter())
        .chain(function.order_by.iter().map(|order| &order.expr))
        .collect()
}

fn select_item_uses_fulltext(item: &SelectItem) -> bool {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => false,
        SelectItem::Function { function, .. } => function_uses_fulltext(function),
        SelectItem::Expr { expr, .. } => expr_uses_fulltext(expr),
        SelectItem::WindowFunction { function, .. } => window_function_expressions(function)
            .into_iter()
            .any(expr_uses_fulltext),
    }
}

fn select_item_uses_vector(item: &SelectItem) -> bool {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => false,
        SelectItem::Function { function, .. } => function_uses_vector(function),
        SelectItem::Expr { expr, .. } => expr_uses_vector(expr),
        SelectItem::WindowFunction { function, .. } => window_function_expressions(function)
            .into_iter()
            .any(expr_uses_vector),
    }
}

fn expr_uses_fulltext(expr: &Expr) -> bool {
    match expr {
        Expr::Function(function) => function_uses_fulltext(function),
        Expr::Binary { left, right, .. } => expr_uses_fulltext(left) || expr_uses_fulltext(right),
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            expr_uses_fulltext(expr)
        }
        Expr::InList { expr, values, .. } => {
            expr_uses_fulltext(expr) || values.iter().any(expr_uses_fulltext)
        }
        Expr::Between {
            expr, low, high, ..
        } => expr_uses_fulltext(expr) || expr_uses_fulltext(low) || expr_uses_fulltext(high),
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Exists(_) => false,
    }
}

fn function_uses_fulltext(function: &FunctionCall) -> bool {
    matches!(
        function.name.to_ascii_lowercase().as_str(),
        "search" | "search_score" | "snippet"
    ) || function.args.iter().any(expr_uses_fulltext)
}

fn expr_uses_vector(expr: &Expr) -> bool {
    match expr {
        Expr::Function(function) => function_uses_vector(function),
        Expr::Binary { left, op, right } => {
            matches!(
                op,
                BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot
            ) || expr_uses_vector(left)
                || expr_uses_vector(right)
        }
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            expr_uses_vector(expr)
        }
        Expr::InList { expr, values, .. } => {
            expr_uses_vector(expr) || values.iter().any(expr_uses_vector)
        }
        Expr::Between {
            expr, low, high, ..
        } => expr_uses_vector(expr) || expr_uses_vector(low) || expr_uses_vector(high),
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Exists(_) => false,
    }
}

fn function_uses_vector(function: &FunctionCall) -> bool {
    matches!(
        function.name.to_ascii_lowercase().as_str(),
        "vector_distance" | "vector_score" | "cosine_distance" | "dot_product"
    ) || function.args.iter().any(expr_uses_vector)
}
