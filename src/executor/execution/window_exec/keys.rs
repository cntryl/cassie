use super::{
    batch, filter, BatchRow, CmpOrdering, NullsOrder, QueryError, SemanticKey, SemanticValue,
    SortDirection, WindowExecutionContext,
};

pub(super) struct WindowPeerKey {
    key: SemanticKey,
    _memory: crate::runtime::QueryMemoryReservation,
}

impl PartialEq for WindowPeerKey {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

pub(super) fn window_peer_key(
    row: &BatchRow,
    order_by: &[crate::sql::ast::OrderExpr],
    context: &WindowExecutionContext<'_>,
) -> Result<WindowPeerKey, QueryError> {
    let parts = order_by
        .iter()
        .map(|order| {
            filter::evaluate_expr_value(
                row,
                &order.expr,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
                None,
            )
            .and_then(|value| {
                Ok(crate::executor::array_order::key(
                    row,
                    &order.expr,
                    &value,
                    context.user_functions,
                    Some(context.controls),
                )?)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let key = SemanticKey::from_semantic_values(parts);
    let memory = context
        .controls
        .reserve_query_memory(key.estimated_bytes())?;
    Ok(WindowPeerKey {
        key,
        _memory: memory,
    })
}

pub(super) struct WindowSortPart {
    pub(super) value: SemanticValue,
    pub(super) direction: SortDirection,
    pub(super) nulls: Option<NullsOrder>,
}

pub(super) struct WindowSortKey {
    pub(super) parts: Vec<WindowSortPart>,
    pub(super) tie_key: String,
}

pub(super) fn window_sort_key(
    row: &BatchRow,
    order_by: &[crate::sql::ast::OrderExpr],
    context: &WindowExecutionContext<'_>,
) -> Result<WindowSortKey, QueryError> {
    let parts = order_by
        .iter()
        .map(|order| {
            filter::evaluate_expr_value(
                row,
                &order.expr,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
                None,
            )
            .and_then(|value| {
                Ok(WindowSortPart {
                    value: crate::executor::array_order::key(
                        row,
                        &order.expr,
                        &value,
                        context.user_functions,
                        Some(context.controls),
                    )?,
                    direction: order.direction.clone(),
                    nulls: order.nulls,
                })
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(WindowSortKey {
        parts,
        tie_key: batch::row_tie_key(row),
    })
}

pub(super) fn compare_window_sort_keys(left: &WindowSortKey, right: &WindowSortKey) -> CmpOrdering {
    for (left, right) in left.parts.iter().zip(&right.parts) {
        let cmp = compare_window_sort_parts(left, right);
        if cmp != CmpOrdering::Equal {
            return cmp;
        }
    }
    left.tie_key.cmp(&right.tie_key)
}

fn compare_window_sort_parts(left: &WindowSortPart, right: &WindowSortPart) -> CmpOrdering {
    let left_null = left.value.is_null();
    let right_null = right.value.is_null();
    if left_null != right_null {
        let nulls = left.nulls.unwrap_or(match &left.direction {
            SortDirection::Asc => NullsOrder::Last,
            SortDirection::Desc => NullsOrder::First,
        });
        return match (left_null, nulls) {
            (true, NullsOrder::First) | (false, NullsOrder::Last) => CmpOrdering::Less,
            (true, NullsOrder::Last) | (false, NullsOrder::First) => CmpOrdering::Greater,
        };
    }

    let cmp = left.value.cmp(&right.value);
    match &left.direction {
        SortDirection::Asc => cmp,
        SortDirection::Desc => cmp.reverse(),
    }
}
