use crate::catalog::{CollectionCardinalityStats, IndexKind, IndexMeta};
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{
    BinaryOp, Expr, FunctionCall, JoinKind, QuerySource, SelectItem, WindowFunctionCall,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::hash::BuildHasher;

#[path = "physical/adaptive.rs"]
mod adaptive;
#[path = "physical/aggregate_accel.rs"]
mod aggregate_accel;
#[path = "physical/column_batches.rs"]
mod column_batches;
#[path = "physical/cost.rs"]
mod cost;
#[path = "physical/feature_flags.rs"]
mod feature_flags;
#[path = "physical/index_selection.rs"]
mod index_selection;
#[path = "physical/join_paths.rs"]
mod join_paths;
#[path = "physical/read_paths.rs"]
mod read_paths;
#[path = "physical/scalar_paths.rs"]
mod scalar_paths;
#[path = "physical/time_series.rs"]
mod time_series;

pub(crate) use adaptive::select_adaptive_read_operator;
pub use adaptive::{AdaptivePlanDiagnostics, OperatorFeedbackPlanDiagnostics};
use aggregate_accel::plan_supports_aggregate_acceleration;
use column_batches::column_batch_index;
use feature_flags::{
    function_uses_fulltext, function_uses_vector, plan_expressions, plan_uses_fulltext,
    plan_uses_vector,
};
pub(crate) use index_selection::{
    read_operator_selection, ReadOperatorCandidate, ReadOperatorSelection,
};
pub(crate) use scalar_paths::{scalar_index_plan_shape, ScalarIndexPlanPath, ScalarIndexPlanShape};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    IndexSeek,
    PrefixScan,
    RangeScan,
    OrderedBoundedScan,
    GraphAdjacency,
    RuntimeJoin,
}

impl ReadAccessPath {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::CollectionScan => "collection_scan",
            Self::PointLookup => "point_lookup",
            Self::IndexSeek => "index_seek",
            Self::PrefixScan => "prefix_scan",
            Self::RangeScan => "range_scan",
            Self::OrderedBoundedScan => "ordered_bounded_scan",
            Self::GraphAdjacency => "graph_adjacency",
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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EarlyStopMode {
    #[default]
    None,
    PointLookup,
    ScanLimit,
    Exists,
    StorageTopK,
    Keyset,
}

impl EarlyStopMode {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::PointLookup => "point_lookup",
            Self::ScanLimit => "scan_limit",
            Self::Exists => "exists",
            Self::StorageTopK => "storage_top_k",
            Self::Keyset => "keyset",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysicalPlan {
    pub collection: String,
    pub operators: Vec<Operator>,
    pub logical: LogicalPlan,
    #[serde(default)]
    pub collection_schema: Option<crate::catalog::CollectionSchema>,
    pub estimates: PlanEstimates,
    #[serde(default)]
    pub operator_feedback: OperatorFeedbackPlanDiagnostics,
    #[serde(default)]
    pub adaptive_plan: AdaptivePlanDiagnostics,
    pub predicate_pushdown: bool,
    pub projected_scan_fields: Vec<String>,
    pub scan_limit: Option<usize>,
    pub selected_index: Option<String>,
    pub covered_index: bool,
    pub column_batch_index: Option<String>,
    pub top_k: bool,
    pub top_k_limit: Option<usize>,
    pub join_strategy: Option<String>,
    #[serde(default)]
    pub join_keys: Vec<String>,
    #[serde(default)]
    pub join_sort_required: bool,
    #[serde(default)]
    pub join_fallback_reason: Option<String>,
    #[serde(default)]
    pub vectorized_join_candidate: bool,
    #[serde(default)]
    pub vectorized_join_fallback_reason: Option<String>,
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
    pub early_stop: EarlyStopMode,
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

#[must_use]
pub fn build(plan: LogicalPlan) -> PhysicalPlan {
    let cardinality_stats = std::collections::HashMap::<String, CollectionCardinalityStats>::new();
    build_with_indexes(plan, &[], &cardinality_stats)
}

#[must_use]
pub fn build_with_indexes<S: BuildHasher>(
    plan: LogicalPlan,
    indexes: &[IndexMeta],
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats, S>,
) -> PhysicalPlan {
    let selected_index = index_selection::base_selected_index(&plan, indexes, cardinality_stats);
    build_with_selection(
        plan,
        indexes,
        cardinality_stats,
        selected_index,
        OperatorFeedbackPlanDiagnostics::default(),
        AdaptivePlanDiagnostics::default(),
    )
}

pub(crate) fn build_with_selection<S: BuildHasher>(
    plan: LogicalPlan,
    indexes: &[IndexMeta],
    cardinality_stats: &std::collections::HashMap<String, CollectionCardinalityStats, S>,
    selected_index: Option<String>,
    operator_feedback: OperatorFeedbackPlanDiagnostics,
    adaptive_plan: AdaptivePlanDiagnostics,
) -> PhysicalPlan {
    if plan.command.is_some() {
        return PhysicalPlan {
            collection: plan.collection.clone(),
            operators: Vec::new(),
            logical: plan,
            collection_schema: None,
            estimates: PlanEstimates::default(),
            operator_feedback,
            adaptive_plan,
            predicate_pushdown: false,
            projected_scan_fields: Vec::new(),
            scan_limit: None,
            selected_index: None,
            covered_index: false,
            column_batch_index: None,
            top_k: false,
            top_k_limit: None,
            join_strategy: None,
            join_keys: Vec::new(),
            join_sort_required: false,
            join_fallback_reason: None,
            vectorized_join_candidate: false,
            vectorized_join_fallback_reason: None,
            parallel_aggregate_candidate: false,
            aggregate_acceleration: false,
            access_path: ReadAccessPath::Unknown,
            access_path_reason: "command-path".to_string(),
            fallback_reason: Some("command".to_string()),
            pagination_strategy: PaginationStrategy::None,
            top_k_mode: TopKMode::None,
            early_stop: EarlyStopMode::None,
            projection_shape: ProjectionShape::Unknown,
        };
    }

    let predicate_pushdown = plan_supports_predicate_pushdown(&plan);
    let projected_scan_fields = projected_scan_fields(&plan).unwrap_or_default();
    let scan_limit = scan_limit(&plan, &projected_scan_fields);
    let covered_index = selected_index
        .as_deref()
        .and_then(|name| indexes.iter().find(|index| index.name == name))
        .is_some_and(|index| plan_is_covered_by_index(&plan, index));
    let column_batch_index = column_batch_index(&plan, indexes);
    let top_k_limit = top_k_limit(&plan);
    let top_k = top_k_limit.is_some();
    let join_strategy = join_paths::join_strategy(&plan);
    let join_keys = join_paths::join_keys(&plan);
    let join_sort_required = join_paths::join_sort_required(&plan, join_strategy.as_deref());
    let join_fallback_reason = join_paths::join_fallback_reason(&plan, join_strategy.as_deref());
    let vectorized_join_candidate = join_paths::vectorized_join_candidate(&plan);
    let vectorized_join_fallback_reason = join_paths::vectorized_join_fallback_reason(&plan);
    let parallel_aggregate_candidate = plan_supports_parallel_aggregation(&plan);
    let aggregate_acceleration = plan_supports_aggregate_acceleration(&plan, indexes);
    let access_path =
        read_paths::determine_read_access_path(&plan, indexes, selected_index.as_deref());
    let access_path_reason = read_paths::read_access_path_reason(&plan, &access_path);
    let fallback_reason = read_paths::read_access_path_fallback_reason(
        &plan,
        &access_path,
        selected_index.as_deref(),
    );
    let pagination_strategy = read_paths::determine_pagination_strategy(&plan, &access_path);
    let top_k_mode = read_paths::determine_top_k_mode(&plan, &access_path);
    let early_stop =
        read_paths::determine_early_stop(&plan, &access_path, &pagination_strategy, &top_k_mode);
    let projection_shape = read_paths::determine_projection_shape(&plan);
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
        collection_schema: None,
        estimates,
        operator_feedback,
        adaptive_plan,
        predicate_pushdown,
        projected_scan_fields,
        scan_limit,
        selected_index,
        covered_index,
        column_batch_index,
        top_k,
        top_k_limit,
        join_strategy,
        join_keys,
        join_sort_required,
        join_fallback_reason,
        vectorized_join_candidate,
        vectorized_join_fallback_reason,
        parallel_aggregate_candidate,
        aggregate_acceleration,
        access_path,
        access_path_reason,
        fallback_reason,
        pagination_strategy,
        top_k_mode,
        early_stop,
        projection_shape,
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
            (
                Expr::Column(_),
                Expr::StringLiteral(_)
                    | Expr::NumberLiteral(_)
                    | Expr::BoolLiteral(_)
                    | Expr::Null
                    | Expr::Param(_)
            ) | (
                Expr::StringLiteral(_)
                    | Expr::NumberLiteral(_)
                    | Expr::BoolLiteral(_)
                    | Expr::Null
                    | Expr::Param(_),
                Expr::Column(_)
            )
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
        QuerySource::Collection(_)
        | QuerySource::Cte(_)
        | QuerySource::TableFunction { .. }
        | QuerySource::SingleRow => false,
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
