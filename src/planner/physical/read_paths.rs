use super::{
    is_row_id_column, join_paths, projected_scan_fields, scalar_index_plan_shape, scan_limit,
    source_contains_join, BinaryOp, EarlyStopMode, Expr, IndexMeta, LogicalPlan,
    PaginationStrategy, ProjectionShape, QuerySource, ReadAccessPath, ScalarIndexPlanPath,
    ScalarIndexPlanShape, SelectItem, TopKMode,
};

pub(super) fn determine_read_access_path(
    plan: &LogicalPlan,
    indexes: &[IndexMeta],
    selected_index: Option<&str>,
) -> ReadAccessPath {
    if source_contains_join(&plan.source) {
        return ReadAccessPath::RuntimeJoin;
    }

    if matches!(plan.source, QuerySource::TableFunction { .. }) {
        return ReadAccessPath::GraphAdjacency;
    }

    if is_row_id_lookup_query(plan) {
        return ReadAccessPath::PointLookup;
    }

    if let Some(shape) = selected_index
        .and_then(|name| indexes.iter().find(|index| index.name == name))
        .and_then(|index| scalar_index_plan_shape(plan, index))
    {
        return match shape.path {
            ScalarIndexPlanPath::IndexSeek => ReadAccessPath::IndexSeek,
            ScalarIndexPlanPath::PrefixScan => ReadAccessPath::PrefixScan,
            ScalarIndexPlanPath::RangeScan => ReadAccessPath::RangeScan,
            ScalarIndexPlanPath::OrderedBoundedScan => ReadAccessPath::OrderedBoundedScan,
        };
    }

    if matches!(&plan.source, QuerySource::Collection(_)) {
        return ReadAccessPath::CollectionScan;
    }

    ReadAccessPath::Unknown
}

pub(super) fn determine_pagination_strategy(
    plan: &LogicalPlan,
    access_path: &ReadAccessPath,
) -> PaginationStrategy {
    let offset = plan.offset.unwrap_or(0);
    if is_row_id_keyset_candidate(plan) {
        return PaginationStrategy::Keyset;
    }

    if plan.limit.is_none() {
        return if offset > 0 {
            PaginationStrategy::Offset
        } else {
            PaginationStrategy::None
        };
    }

    if offset > 0 {
        if matches!(
            access_path,
            ReadAccessPath::IndexSeek
                | ReadAccessPath::PrefixScan
                | ReadAccessPath::RangeScan
                | ReadAccessPath::OrderedBoundedScan
        ) {
            PaginationStrategy::Offset
        } else {
            PaginationStrategy::DegradedOffset
        }
    } else {
        PaginationStrategy::Limit
    }
}

pub(super) fn determine_top_k_mode(plan: &LogicalPlan, access_path: &ReadAccessPath) -> TopKMode {
    if !is_heap_top_k_candidate(plan) {
        return TopKMode::None;
    }

    if is_storage_top_k_candidate(plan) || matches!(access_path, ReadAccessPath::OrderedBoundedScan)
    {
        return TopKMode::Storage;
    }

    if is_row_id_ordered_page_candidate(plan) {
        return TopKMode::None;
    }

    TopKMode::Heap
}

pub(super) fn determine_early_stop(
    plan: &LogicalPlan,
    access_path: &ReadAccessPath,
    pagination_strategy: &PaginationStrategy,
    top_k_mode: &TopKMode,
    scalar_shape: Option<&ScalarIndexPlanShape>,
) -> EarlyStopMode {
    if matches!(access_path, ReadAccessPath::PointLookup) {
        return EarlyStopMode::PointLookup;
    }

    if scalar_shape.is_some_and(|shape| !shape.order_satisfied && !plan.order.is_empty()) {
        return EarlyStopMode::None;
    }

    if matches!(
        access_path,
        ReadAccessPath::IndexSeek | ReadAccessPath::PrefixScan | ReadAccessPath::RangeScan
    ) && plan.limit.is_some()
        && plan.offset.is_none_or(|offset| offset <= 0)
    {
        return EarlyStopMode::ScanLimit;
    }

    if plan.filter.as_ref().is_some_and(|filter| {
        join_paths::expr_contains_exists(filter) || join_paths::expr_contains_not_exists(filter)
    }) {
        return EarlyStopMode::Exists;
    }

    if matches!(pagination_strategy, PaginationStrategy::Keyset) {
        return EarlyStopMode::Keyset;
    }

    if matches!(top_k_mode, TopKMode::Storage) {
        return EarlyStopMode::StorageTopK;
    }

    if matches!(top_k_mode, TopKMode::Heap) {
        return EarlyStopMode::None;
    }

    if supports_scan_limit_early_stop(plan) {
        return EarlyStopMode::ScanLimit;
    }

    if matches!(
        pagination_strategy,
        PaginationStrategy::Offset | PaginationStrategy::DegradedOffset
    ) {
        return EarlyStopMode::None;
    }

    EarlyStopMode::None
}

pub(super) fn determine_projection_shape(plan: &LogicalPlan) -> ProjectionShape {
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

pub(super) fn read_access_path_reason(plan: &LogicalPlan, access_path: &ReadAccessPath) -> String {
    match access_path {
        ReadAccessPath::Unknown => {
            if plan.command.is_some() {
                "command-path".to_string()
            } else {
                "unsupported-plan-shape".to_string()
            }
        }
        ReadAccessPath::CollectionScan => {
            if is_row_id_storage_top_k_candidate(plan) {
                "row-key-top-k".to_string()
            } else if is_row_id_keyset_candidate(plan) {
                "row-key-keyset".to_string()
            } else if is_row_id_ordered_page_candidate(plan) {
                "row-key-ordered-page".to_string()
            } else {
                "collection-scan".to_string()
            }
        }
        ReadAccessPath::PointLookup => "point-lookup-id".to_string(),
        ReadAccessPath::IndexSeek => "scalar-index-seek".to_string(),
        ReadAccessPath::PrefixScan => "scalar-index-prefix".to_string(),
        ReadAccessPath::RangeScan => "scalar-index-range".to_string(),
        ReadAccessPath::OrderedBoundedScan => "scalar-index-ordered-bounded".to_string(),
        ReadAccessPath::GraphAdjacency => "graph-table-function".to_string(),
        ReadAccessPath::RuntimeJoin => "runtime-join".to_string(),
    }
}

pub(super) fn read_access_path_fallback_reason(
    plan: &LogicalPlan,
    access_path: &ReadAccessPath,
    selected_index: Option<&str>,
) -> Option<String> {
    match access_path {
        ReadAccessPath::CollectionScan => {
            if matches!(
                determine_pagination_strategy(plan, access_path),
                PaginationStrategy::DegradedOffset | PaginationStrategy::Offset
            ) {
                Some("offset-degraded".to_string())
            } else if selected_index.is_some() {
                Some(if plan.order.is_empty() {
                    "index-bounds-unavailable".to_string()
                } else {
                    "index-order-proof-missing".to_string()
                })
            } else {
                None
            }
        }
        ReadAccessPath::RuntimeJoin => Some("runtime-join-required".to_string()),
        _ => None,
    }
}

fn supports_scan_limit_early_stop(plan: &LogicalPlan) -> bool {
    if plan.filter.is_some()
        || !plan.order.is_empty()
        || !matches!(plan.source, QuerySource::Collection(_))
        || !is_row_projection(plan)
    {
        return false;
    }

    scan_limit(plan, &projected_scan_fields(plan).unwrap_or_default()).is_some()
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
        && plan.offset.is_none_or(|offset| offset <= 0)
}

fn is_row_id_ordering(plan: &LogicalPlan) -> bool {
    plan.order.len() == 1
        && plan.order[0].nulls.is_none()
        && matches!(&plan.order[0].expr, Expr::Column(column) if is_row_id_column(column))
}

fn is_row_id_range_filter(expr: &Expr) -> bool {
    let Expr::Binary { left, op, right } = expr else {
        return false;
    };

    matches!(
        op,
        BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte
    ) && matches!(
        (left.as_ref(), right.as_ref()),
        (Expr::Column(column), other) | (other, Expr::Column(column))
            if is_row_id_column(column)
                && matches!(
                    other,
                    Expr::StringLiteral(_)
                        | Expr::BoolLiteral(_)
                        | Expr::NumberLiteral(_)
                        | Expr::Null
                        | Expr::Param(_)
                )
    )
}

fn is_row_id_ordered_page_candidate(plan: &LogicalPlan) -> bool {
    if plan.command.is_some()
        || !plan.ctes.is_empty()
        || plan.distinct
        || !plan.distinct_on.is_empty()
        || !plan.group_by.is_empty()
        || plan.having.is_some()
        || plan.set.is_some()
        || !matches!(plan.source, QuerySource::Collection(_))
        || !is_row_projection(plan)
        || !is_row_id_ordering(plan)
        || plan.limit.is_none()
    {
        return false;
    }

    match plan.filter.as_ref() {
        None => true,
        Some(filter) => is_row_id_range_filter(filter),
    }
}

fn is_row_id_keyset_candidate(plan: &LogicalPlan) -> bool {
    is_row_id_ordered_page_candidate(plan)
        && plan.filter.as_ref().is_some_and(is_row_id_range_filter)
        && plan.offset.is_none_or(|offset| offset <= 0)
}

fn is_row_id_storage_top_k_candidate(plan: &LogicalPlan) -> bool {
    is_row_id_ordered_page_candidate(plan)
        && plan.filter.is_none()
        && plan.offset.is_none_or(|offset| offset <= 0)
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
    let ((Expr::Column(column), other) | (other, Expr::Column(column))) = (lhs, rhs) else {
        return false;
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
        && plan.offset.is_none_or(|offset| offset <= 0)
        && !plan.distinct
        && plan.distinct_on.is_empty()
        && plan.group_by.is_empty()
        && plan.having.is_none()
        && plan.set.is_none()
        && plan
            .projection
            .iter()
            .all(|item| matches!(item, SelectItem::Column { .. }))
        && is_row_id_ordering(plan)
        && plan.ctes.is_empty()
}
