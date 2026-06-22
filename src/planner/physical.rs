use crate::catalog::{CollectionCardinalityStats, IndexKind, IndexMeta};
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{
    BinaryOp, Expr, FunctionCall, JoinKind, QuerySource, SelectItem, WindowFunctionCall,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[path = "physical/aggregate_accel.rs"]
mod aggregate_accel;
#[path = "physical/column_batches.rs"]
mod column_batches;
#[path = "physical/cost.rs"]
mod cost;
#[path = "physical/feature_flags.rs"]
mod feature_flags;
#[path = "physical/time_series.rs"]
mod time_series;

use aggregate_accel::plan_supports_aggregate_acceleration;
use column_batches::column_batch_index;
use feature_flags::{
    function_uses_fulltext, function_uses_vector, plan_expressions, plan_uses_fulltext,
    plan_uses_vector,
};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadAccessPath {
    #[default]
    Unknown,
    CollectionScan,
    PointLookup,
    RuntimeJoin,
}

impl ReadAccessPath {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::CollectionScan => "collection_scan",
            Self::PointLookup => "point_lookup",
            Self::RuntimeJoin => "runtime_join",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaginationStrategy {
    #[default]
    None,
    Limit,
    Offset,
    Keyset,
    DegradedOffset,
}

impl PaginationStrategy {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Limit => "limit",
            Self::Offset => "offset",
            Self::Keyset => "keyset",
            Self::DegradedOffset => "degraded_offset",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TopKMode {
    #[default]
    None,
    Heap,
    Storage,
}

impl TopKMode {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Heap => "heap",
            Self::Storage => "storage",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionShape {
    #[default]
    Unknown,
    RuntimeJoinDegraded,
    Collection,
    MaterializedProjection,
    Other,
}

impl ProjectionShape {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::RuntimeJoinDegraded => "runtime_join_degraded",
            Self::Collection => "collection",
            Self::MaterializedProjection => "materialized_projection",
            Self::Other => "other",
        }
    }
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
    pub column_batch_index: Option<String>,
    pub top_k: bool,
    pub top_k_limit: Option<usize>,
    pub join_strategy: Option<String>,
    pub parallel_aggregate_candidate: bool,
    pub aggregate_acceleration: bool,
    #[serde(default)]
    pub access_path: ReadAccessPath,
    #[serde(default)]
    pub access_path_reason: String,
    #[serde(default)]
    pub fallback_reason: Option<String>,
    #[serde(default)]
    pub pagination_strategy: PaginationStrategy,
    #[serde(default)]
    pub top_k_mode: TopKMode,
    #[serde(default)]
    pub projection_shape: ProjectionShape,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlanEstimates {
    pub cost_model_version: u32,
    pub scan_rows: u64,
    pub index_rows: u64,
    pub join_rows: u64,
    pub search_rows: u64,
    pub vector_rows: u64,
    pub aggregate_rows: u64,
    pub scan_cost: u64,
    pub index_cost: u64,
    pub selected_cost: u64,
    pub cost_source: String,
    pub rejected_alternatives: Vec<String>,
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
            column_batch_index: None,
            top_k: false,
            top_k_limit: None,
            join_strategy: None,
            parallel_aggregate_candidate: false,
            aggregate_acceleration: false,
            access_path: ReadAccessPath::Unknown,
            access_path_reason: "command-path".to_string(),
            fallback_reason: Some("command".to_string()),
            pagination_strategy: PaginationStrategy::None,
            top_k_mode: TopKMode::None,
            projection_shape: ProjectionShape::Unknown,
        };
    }

    let predicate_pushdown = plan_supports_predicate_pushdown(&plan);
    let projected_scan_fields = projected_scan_fields(&plan).unwrap_or_default();
    let scan_limit = scan_limit(&plan, &projected_scan_fields);
    let selected_index = selected_index(&plan, indexes.as_slice(), cardinality_stats);
    let covered_index = selected_index
        .as_deref()
        .and_then(|name| indexes.iter().find(|index| index.name == name))
        .is_some_and(|index| plan_is_covered_by_index(&plan, index));
    let column_batch_index = column_batch_index(&plan, indexes.as_slice());
    let top_k_limit = top_k_limit(&plan);
    let top_k = top_k_limit.is_some();
    let join_strategy = join_strategy(&plan);
    let parallel_aggregate_candidate = plan_supports_parallel_aggregation(&plan);
    let aggregate_acceleration = plan_supports_aggregate_acceleration(&plan, indexes.as_slice());
    let access_path = determine_read_access_path(&plan);
    let access_path_reason = read_access_path_reason(&plan, &access_path);
    let fallback_reason = read_access_path_fallback_reason(&plan, &access_path);
    let pagination_strategy = determine_pagination_strategy(&plan);
    let top_k_mode = determine_top_k_mode(&plan);
    let projection_shape = determine_projection_shape(&plan);
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
        column_batch_index,
        top_k,
        top_k_limit,
        join_strategy,
        parallel_aggregate_candidate,
        aggregate_acceleration,
        access_path,
        access_path_reason,
        fallback_reason,
        pagination_strategy,
        top_k_mode,
        projection_shape,
    }
}

fn determine_read_access_path(plan: &LogicalPlan) -> ReadAccessPath {
    if source_contains_join(&plan.source) {
        return ReadAccessPath::RuntimeJoin;
    }

    if is_row_id_lookup_query(plan) {
        return ReadAccessPath::PointLookup;
    }

    if matches!(&plan.source, QuerySource::Collection(_)) {
        return ReadAccessPath::CollectionScan;
    }

    ReadAccessPath::Unknown
}

fn determine_pagination_strategy(plan: &LogicalPlan) -> PaginationStrategy {
    let offset = plan.offset.unwrap_or(0);
    if plan.limit.is_none() {
        return if offset > 0 {
            PaginationStrategy::DegradedOffset
        } else {
            PaginationStrategy::None
        };
    }

    if offset > 0 {
        PaginationStrategy::DegradedOffset
    } else {
        PaginationStrategy::Limit
    }
}

fn determine_top_k_mode(plan: &LogicalPlan) -> TopKMode {
    if !is_heap_top_k_candidate(plan) {
        return TopKMode::None;
    }

    if is_storage_top_k_candidate(plan) {
        return TopKMode::Storage;
    }

    TopKMode::Heap
}

fn determine_projection_shape(plan: &LogicalPlan) -> ProjectionShape {
    if source_contains_join(&plan.source) {
        return ProjectionShape::RuntimeJoinDegraded;
    }

    if is_row_projection(plan) {
        if plan.filter.is_some() {
            return ProjectionShape::MaterializedProjection;
        }
        return ProjectionShape::Collection;
    }

    ProjectionShape::Other
}

fn read_access_path_reason(plan: &LogicalPlan, access_path: &ReadAccessPath) -> String {
    match access_path {
        ReadAccessPath::Unknown => {
            if plan.command.is_some() {
                "command-path".to_string()
            } else {
                "unsupported-plan-shape".to_string()
            }
        }
        ReadAccessPath::CollectionScan => "collection-scan".to_string(),
        ReadAccessPath::PointLookup => "point-lookup-id".to_string(),
        ReadAccessPath::RuntimeJoin => "runtime-join".to_string(),
    }
}

fn read_access_path_fallback_reason(
    plan: &LogicalPlan,
    access_path: &ReadAccessPath,
) -> Option<String> {
    match access_path {
        ReadAccessPath::CollectionScan => {
            if plan.offset.is_some() {
                Some("offset-degraded".to_string())
            } else {
                None
            }
        }
        ReadAccessPath::RuntimeJoin => Some("runtime-join-required".to_string()),
        _ => None,
    }
}

fn is_row_projection(plan: &LogicalPlan) -> bool {
    !plan.projection.is_empty()
        && plan
            .projection
            .iter()
            .all(|item| matches!(item, SelectItem::Column { .. }))
}

fn is_row_id_lookup_query(plan: &LogicalPlan) -> bool {
    if plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !matches!(plan.source, QuerySource::Collection(_))
    {
        return false;
    }

    let Some(filter) = plan.filter.as_ref() else {
        return false;
    };

    is_id_point_lookup_filter(filter)
        && is_row_projection(plan)
        && !plan.offset.is_some_and(|offset| offset > 0)
}

fn is_id_point_lookup_filter(expr: &Expr) -> bool {
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = expr
    else {
        return false;
    };

    let (lhs, rhs) = (left.as_ref(), right.as_ref());
    let (column, other) = match (lhs, rhs) {
        (Expr::Column(column), other) => (column, other),
        (other, Expr::Column(column)) => (column, other),
        _ => return false,
    };

    if !is_row_id_column(column) {
        return false;
    }

    matches!(
        other,
        Expr::StringLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::Null
            | Expr::Param(_)
    )
}

fn is_heap_top_k_candidate(plan: &LogicalPlan) -> bool {
    !plan.order.is_empty() && plan.limit.is_some() && plan.set.is_none()
}

fn is_storage_top_k_candidate(plan: &LogicalPlan) -> bool {
    is_heap_top_k_candidate(plan)
        && plan.filter.is_none()
        && !plan.distinct
        && plan.distinct_on.is_empty()
        && plan.group_by.is_empty()
        && plan.having.is_none()
        && plan.set.is_none()
        && plan
            .projection
            .iter()
            .all(|item| matches!(item, SelectItem::Column { .. }))
        && plan.order.len() == 1
        && matches!(plan.order[0].expr, Expr::Column(_))
        && plan.ctes.is_empty()
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

pub(super) fn is_equi_join_predicate(expr: &Expr) -> bool {
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

fn selected_index(
    plan: &LogicalPlan,
    indexes: &[IndexMeta],
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> Option<String> {
    let QuerySource::Collection(collection) = &plan.source else {
        return None;
    };
    let filter = plan.filter.as_ref()?;
    let equality_fields = equality_filter_fields(filter);
    let equality_expressions = equality_filter_expressions(filter);
    let scalar = indexes
        .iter()
        .filter(|index| index.collection == *collection && index.kind == IndexKind::Scalar)
        .filter(|index| partial_index_matches_query(plan.filter.as_ref(), index.predicate.as_ref()))
        .filter(|index| {
            let field_match = index
                .normalized_fields()
                .iter()
                .all(|field| equality_fields.contains(&field.to_ascii_lowercase()));
            let expression_match = index
                .normalized_expressions()
                .iter()
                .all(|expression| equality_expressions.contains(expression));
            field_match && expression_match
        })
        .min_by(|left, right| {
            index_estimate(collection, left, cardinality_stats)
                .cmp(&index_estimate(collection, right, cardinality_stats))
                .then_with(|| {
                    let right_specificity =
                        right.normalized_fields().len() + right.normalized_expressions().len();
                    let left_specificity =
                        left.normalized_fields().len() + left.normalized_expressions().len();
                    right_specificity.cmp(&left_specificity)
                })
                .then_with(|| left.name.cmp(&right.name))
        })
        .map(|index| index.name.clone());
    scalar.or_else(|| time_series::selected_time_series_index(collection, filter, indexes))
}

fn index_estimate(
    collection: &str,
    index: &IndexMeta,
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats>,
) -> u64 {
    cardinality_stats
        .get(collection)
        .filter(|stats| stats.hydrated)
        .and_then(|stats| {
            stats.index_cardinality(&CollectionCardinalityStats::index_key(
                &index.kind,
                &index.name,
            ))
        })
        .unwrap_or(u64::MAX)
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

fn equality_filter_expressions(expr: &Expr) -> BTreeSet<String> {
    let mut expressions = BTreeSet::new();
    collect_equality_filter_expressions(expr, &mut expressions);
    expressions
}

fn collect_equality_filter_expressions(expr: &Expr, expressions: &mut BTreeSet<String>) {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_equality_filter_expressions(left, expressions);
            collect_equality_filter_expressions(right, expressions);
        }
        Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } => {
            if expr_has_column(left)
                && !matches!(left.as_ref(), Expr::Column(_))
                && expr_is_constant(right)
            {
                if let Ok(serialized) = serde_json::to_string(left.as_ref()) {
                    expressions.insert(serialized);
                }
            }
            if expr_has_column(right)
                && !matches!(right.as_ref(), Expr::Column(_))
                && expr_is_constant(left)
            {
                if let Ok(serialized) = serde_json::to_string(right.as_ref()) {
                    expressions.insert(serialized);
                }
            }
        }
        _ => {}
    }
}

fn expr_is_constant(expr: &Expr) -> bool {
    match expr {
        Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Param(_) => true,
        Expr::Column(_) | Expr::Exists(_) => false,
        Expr::Binary { left, right, .. } => expr_is_constant(left) && expr_is_constant(right),
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            expr_is_constant(expr)
        }
        Expr::InList { expr, values, .. } => {
            expr_is_constant(expr) && values.iter().all(expr_is_constant)
        }
        Expr::Between {
            expr, low, high, ..
        } => expr_is_constant(expr) && expr_is_constant(low) && expr_is_constant(high),
        Expr::Function(function) => function.args.iter().all(expr_is_constant),
    }
}

fn expr_has_column(expr: &Expr) -> bool {
    match expr {
        Expr::Column(_) => true,
        Expr::Binary { left, right, .. } => expr_has_column(left) || expr_has_column(right),
        Expr::IsNull { expr, .. } | Expr::Not { expr } | Expr::Cast { expr, .. } => {
            expr_has_column(expr)
        }
        Expr::InList { expr, values, .. } => {
            expr_has_column(expr) || values.iter().any(expr_has_column)
        }
        Expr::Between {
            expr, low, high, ..
        } => expr_has_column(expr) || expr_has_column(low) || expr_has_column(high),
        Expr::Function(function) => function.args.iter().any(expr_has_column),
        Expr::Exists(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null
        | Expr::Param(_) => false,
    }
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
    let order_columns = projected_order_columns(plan)?;

    let mut fields = Vec::new();
    for column in projection_columns
        .into_iter()
        .chain(filter_columns)
        .chain(order_columns)
    {
        if is_row_id_column(&column) || fields.iter().any(|field: &String| field == &column) {
            continue;
        }
        fields.push(column);
    }
    Some(fields)
}

fn projected_order_columns(plan: &LogicalPlan) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    for order in &plan.order {
        let Expr::Column(column) = &order.expr else {
            return None;
        };
        if !fields.iter().any(|field: &String| field == column) {
            fields.push(column.clone());
        }
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

fn plan_supports_parallel_aggregation(plan: &LogicalPlan) -> bool {
    plan_uses_aggregate(plan)
        && !plan.distinct
        && plan.distinct_on.is_empty()
        && plan.set.is_none()
        && !plan
            .projection
            .iter()
            .any(|item| matches!(item, SelectItem::WindowFunction { .. }))
        && aggregate_functions_supported(plan)
        && plan_expressions(plan).all(expr_supports_parallel_aggregation)
}

fn aggregate_functions_supported(plan: &LogicalPlan) -> bool {
    plan.projection
        .iter()
        .filter_map(|item| match item {
            SelectItem::Function { function, .. } => Some(function),
            _ => None,
        })
        .chain(plan.having.iter().flat_map(aggregate_functions_in_expr))
        .chain(
            plan.order
                .iter()
                .flat_map(|order| aggregate_functions_in_expr(&order.expr)),
        )
        .all(|function| {
            matches!(
                function.name.to_ascii_lowercase().as_str(),
                "count" | "sum" | "avg" | "min" | "max"
            )
        })
}

fn aggregate_functions_in_expr(expr: &Expr) -> Vec<&FunctionCall> {
    match expr {
        Expr::Function(function) => {
            let mut functions = function
                .args
                .iter()
                .flat_map(aggregate_functions_in_expr)
                .collect::<Vec<_>>();
            if crate::sql::functions::is_aggregate_function(&function.name) {
                functions.push(function);
            }
            functions
        }
        Expr::Binary { left, right, .. } => {
            let mut functions = aggregate_functions_in_expr(left);
            functions.extend(aggregate_functions_in_expr(right));
            functions
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } | Expr::Not { expr } => {
            aggregate_functions_in_expr(expr)
        }
        Expr::InList { expr, values, .. } => {
            let mut functions = aggregate_functions_in_expr(expr);
            for value in values {
                functions.extend(aggregate_functions_in_expr(value));
            }
            functions
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            let mut functions = aggregate_functions_in_expr(expr);
            functions.extend(aggregate_functions_in_expr(low));
            functions.extend(aggregate_functions_in_expr(high));
            functions
        }
        Expr::Exists(_)
        | Expr::Column(_)
        | Expr::Param(_)
        | Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_) => Vec::new(),
    }
}

fn expr_supports_parallel_aggregation(expr: &Expr) -> bool {
    match expr {
        Expr::Function(function) => {
            if crate::sql::functions::is_aggregate_function(&function.name) {
                matches!(
                    function.name.to_ascii_lowercase().as_str(),
                    "count" | "sum" | "avg" | "min" | "max"
                ) && function.args.iter().all(expr_supports_parallel_aggregation)
            } else {
                !function_uses_fulltext(function)
                    && !function_uses_vector(function)
                    && !matches!(
                        function.name.to_ascii_lowercase().as_str(),
                        "hybrid_score" | "vector_score"
                    )
                    && function.args.iter().all(expr_supports_parallel_aggregation)
            }
        }
        Expr::Binary { left, right, .. } => {
            expr_supports_parallel_aggregation(left) && expr_supports_parallel_aggregation(right)
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } | Expr::Not { expr } => {
            expr_supports_parallel_aggregation(expr)
        }
        Expr::InList { expr, values, .. } => {
            expr_supports_parallel_aggregation(expr)
                && values.iter().all(expr_supports_parallel_aggregation)
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            expr_supports_parallel_aggregation(expr)
                && expr_supports_parallel_aggregation(low)
                && expr_supports_parallel_aggregation(high)
        }
        Expr::Exists(_) => false,
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_) => true,
    }
}
