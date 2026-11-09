use super::{
    filter, BatchRow, CmpOrdering, NullsOrder, QueryError, SemanticKey, SemanticValue,
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
    let bytes = crate::executor::retained_memory::mul(
        order_by.len(),
        std::mem::size_of::<SemanticValue>(),
    )?;
    let mut memory = context.controls.reserve_query_memory(bytes)?;
    let mut parts = Vec::new();
    reserve_exact(&mut parts, order_by.len())?;
    for order in order_by {
        let (value, part_memory) = key_part(row, order, context)?;
        if !part_memory.belongs_to(context.controls) {
            drop(value);
            drop(part_memory);
            return Err(crate::app::CassieError::ResourceLimit(
                "window key owner uses a different query tracker".into(),
            )
            .into());
        }
        memory.absorb(part_memory);
        parts.push(value);
    }
    let key = SemanticKey::from_semantic_values(parts);
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
    pub(super) _memory: Option<crate::runtime::QueryMemoryReservation>,
}

pub(super) fn window_sort_key(
    row: &BatchRow,
    order_by: &[crate::sql::ast::OrderExpr],
    context: &WindowExecutionContext<'_>,
) -> Result<WindowSortKey, QueryError> {
    let bytes = crate::executor::retained_memory::mul(
        order_by.len(),
        std::mem::size_of::<WindowSortPart>(),
    )?;
    let mut memory = context.controls.reserve_query_memory(bytes)?;
    let mut parts = Vec::new();
    reserve_exact(&mut parts, order_by.len())?;
    for order in order_by {
        let (value, part_memory) = key_part(row, order, context)?;
        if !part_memory.belongs_to(context.controls) {
            drop(value);
            drop(part_memory);
            return Err(crate::app::CassieError::ResourceLimit(
                "window key owner uses a different query tracker".into(),
            )
            .into());
        }
        memory.absorb(part_memory);
        parts.push(WindowSortPart {
            value,
            direction: order.direction.clone(),
            nulls: order.nulls,
        });
    }
    let (tie_key, tie_memory) =
        crate::executor::sort::tie::serialize(row, context.controls)?.into_parts();
    if !tie_memory.belongs_to(context.controls) {
        drop(tie_key);
        drop(tie_memory);
        return Err(crate::app::CassieError::ResourceLimit(
            "window tie owner uses a different query tracker".into(),
        )
        .into());
    }
    memory.absorb(tie_memory);
    Ok(WindowSortKey {
        parts,
        tie_key,
        _memory: Some(memory),
    })
}

fn key_part(
    row: &BatchRow,
    order: &crate::sql::ast::OrderExpr,
    context: &WindowExecutionContext<'_>,
) -> Result<(SemanticValue, crate::runtime::QueryMemoryReservation), QueryError> {
    let value = filter::evaluate_expr_value(
        row,
        &order.expr,
        context.params,
        context.search_context,
        context.user_functions,
        context.session,
        None,
    )?;
    let data_type =
        crate::executor::array_order::expression_type(row, &order.expr, context.user_functions);
    let scalar = matches!(
        &value,
        crate::types::Value::Null
            | crate::types::Value::Bool(_)
            | crate::types::Value::Int64(_)
            | crate::types::Value::Float64(_)
            | crate::types::Value::String(_)
    ) && !matches!(data_type.as_ref(), Some(crate::types::DataType::Array(_)));
    let vector = crate::executor::array_order::sort_vector::bound(
        &value,
        data_type.as_ref(),
        context.controls,
    )?
    .is_some();
    let numeric =
        crate::executor::array_order::sort_numeric::bound(&value, data_type.as_ref())?.is_some();
    let bit = crate::executor::array_order::sort_bit::eligible(&value, data_type.as_ref());
    if scalar || vector || numeric || bit {
        return crate::executor::sort::key_part::convert(
            row,
            &order.expr,
            value,
            context.user_functions,
            context.controls,
            || Ok(()),
        );
    }

    // Other shapes retain WINDOW's existing conversion and len-based charge.
    // Their construction and physical capacity remain outside this increment.
    let semantic = crate::executor::array_order::key(
        row,
        &order.expr,
        &value,
        context.user_functions,
        Some(context.controls),
    )?;
    drop(value);
    let memory = context
        .controls
        .reserve_query_memory(semantic.estimated_bytes())?;
    Ok((semantic, memory))
}

fn reserve_exact<T>(values: &mut Vec<T>, count: usize) -> Result<(), QueryError> {
    values
        .try_reserve_exact(count)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if values.capacity() > count {
        return Err(crate::app::CassieError::ResourceLimit(
            "window key directory exceeded its admission".into(),
        )
        .into());
    }
    Ok(())
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
