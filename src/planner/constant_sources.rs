//! Bounded propagation of a pure, single VALUES row after SQL binding.
use super::logical::LogicalPlan;
use crate::app::CassieError;
use crate::executor::retained_memory::{add, data_type_clone_bytes, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{Expr, QuerySource, SelectItem};
use crate::types::DataType;
use std::mem::size_of;

struct Input<'a> {
    cells: &'a [Expr],
    qualifier: Option<String>,
}
impl Input<'_> {
    fn column(&self, name: &str) -> Option<&Expr> {
        let name = match &self.qualifier {
            Some(qualifier) => name.strip_prefix(qualifier)?.strip_prefix('.')?,
            None => name,
        };
        let digits = name.strip_prefix("column")?;
        if digits.starts_with('0') {
            return None;
        }
        let index = digits.parse::<usize>().ok()?.checked_sub(1)?;
        self.cells.get(index)
    }
}

pub(crate) fn inline(
    plan: &mut LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<Option<QueryMemoryReservation>, CassieError> {
    controls.check_interruption()?;
    if plan.command.is_some() || plan.set.is_some() || has_scope_boundary(plan) {
        return Ok(None);
    }
    let Some((cells, alias)) = producer(&plan.source) else {
        return Ok(None);
    };
    if cells.iter().any(|cell| {
        cell.any_descendant_or_self(&mut |expr| {
            matches!(
                expr,
                Expr::Param(_)
                    | Expr::Column(_)
                    | Expr::Function(_)
                    | Expr::Exists(_)
                    | Expr::Cast {
                        data_type: DataType::Char { .. },
                        ..
                    }
            )
        })
    }) {
        return Ok(None);
    }
    // Qualifier encoding and every resulting allocation are admitted before
    // changing the owned plan. A failed compile never publishes a partial plan.
    let qualifier_bytes = alias.map_or(Ok(0), |alias| add(mul(alias.len(), 8)?, 128))?;
    let mut memory = controls.reserve_query_memory(qualifier_bytes)?;
    let input = Input {
        cells,
        qualifier: alias.map(crate::sql::binder::alias_row_qualifier),
    };
    let capacity = plan.projection.iter().try_fold(0_usize, |count, item| {
        add(
            count,
            if matches!(item, SelectItem::Wildcard) {
                input.cells.len()
            } else {
                1
            },
        )
    })?;
    let mut bytes = mul(capacity, 4 * size_of::<SelectItem>())?;
    for item in &plan.projection {
        controls.check_interruption()?;
        bytes = add(bytes, estimate_item(item, &input, controls)?)?;
    }
    for expr in plan
        .filter
        .iter()
        .chain(&plan.having)
        .chain(&plan.group_by)
        .chain(&plan.distinct_on)
    {
        bytes = add(bytes, estimate(expr, &input, controls, 0)?)?;
    }
    for order in &plan.order {
        if !output_alias(&order.expr, &plan.projection) {
            bytes = add(bytes, estimate(&order.expr, &input, controls, 0)?)?;
        }
    }
    memory.try_grow(bytes)?;
    rewrite_projection(&mut plan.projection, &input, controls, capacity)?;
    for expr in plan
        .filter
        .iter_mut()
        .chain(&mut plan.having)
        .chain(&mut plan.group_by)
        .chain(&mut plan.distinct_on)
    {
        rewrite(expr, &input, controls)?;
    }
    for order in &mut plan.order {
        if !output_alias(&order.expr, &plan.projection) {
            rewrite(&mut order.expr, &input, controls)?;
        }
    }
    plan.source = QuerySource::SingleRow;
    Ok(Some(memory))
}

fn rewrite_projection(
    items: &mut Vec<SelectItem>,
    input: &Input<'_>,
    controls: &QueryExecutionControls,
    capacity: usize,
) -> Result<(), CassieError> {
    let mut projection = Vec::with_capacity(capacity);
    for item in std::mem::take(items) {
        match item {
            SelectItem::Wildcard => {
                for (index, cell) in input.cells.iter().enumerate() {
                    controls.check_interruption()?;
                    projection.push(SelectItem::Expr {
                        expr: cell.clone(),
                        alias: Some(format!("column{}", index + 1)),
                    });
                }
            }
            SelectItem::Column { name, alias } => {
                if let Some(cell) = input.column(&name) {
                    let alias = alias.or_else(|| {
                        Some(
                            crate::sql::ColumnIdentifierPath::parse(&name)
                                .map_or(name.clone(), |path| path.declared_name()),
                        )
                    });
                    projection.push(SelectItem::Expr {
                        expr: cell.clone(),
                        alias,
                    });
                } else {
                    projection.push(SelectItem::Column { name, alias });
                }
            }
            SelectItem::Expr { mut expr, alias } => {
                rewrite(&mut expr, input, controls)?;
                projection.push(SelectItem::Expr { expr, alias });
            }
            SelectItem::Function {
                mut function,
                alias,
            } => {
                for expr in function.expressions_mut() {
                    rewrite(expr, input, controls)?;
                }
                projection.push(SelectItem::Function { function, alias });
            }
            SelectItem::WindowFunction { .. } => {
                unreachable!("window boundary checked before mutation")
            }
        }
    }
    *items = projection;
    Ok(())
}

fn producer(source: &QuerySource) -> Option<(&[Expr], Option<&str>)> {
    match source {
        QuerySource::Values { rows } if rows.len() == 1 => Some((&rows[0], None)),
        QuerySource::Aliased { source, alias, .. } => match source.as_ref() {
            QuerySource::Values { rows } if rows.len() == 1 => Some((&rows[0], Some(alias))),
            _ => None,
        },
        _ => None,
    }
}

fn has_scope_boundary(plan: &LogicalPlan) -> bool {
    let exists =
        |expr: &Expr| expr.any_descendant_or_self(&mut |expr| matches!(expr, Expr::Exists(_)));
    plan.projection.iter().any(|item| match item {
        SelectItem::Expr { expr, .. } => exists(expr),
        SelectItem::Function { function, .. } => function.expressions().any(exists),
        SelectItem::WindowFunction { .. } => true,
        _ => false,
    }) || plan
        .filter
        .iter()
        .chain(&plan.having)
        .chain(&plan.group_by)
        .chain(&plan.distinct_on)
        .any(exists)
        || plan.order.iter().any(|order| exists(&order.expr))
}

fn output_alias(expr: &Expr, projection: &[SelectItem]) -> bool {
    let Expr::Column(name) = expr else {
        return false;
    };
    projection.iter().any(|item| {
        let alias = match item {
            SelectItem::Column { alias, .. }
            | SelectItem::Expr { alias, .. }
            | SelectItem::Function { alias, .. } => alias.as_ref(),
            _ => None,
        };
        alias.is_some_and(|alias| {
            crate::sql::ColumnIdentifierPath::from_field_name(alias).lookup_key() == *name
        })
    })
}

fn estimate_item(
    item: &SelectItem,
    input: &Input<'_>,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    match item {
        SelectItem::Wildcard => input.cells.iter().try_fold(0, |bytes, cell| {
            add(bytes, add(clone_bound(cell, controls, 0)?, 104)?)
        }),
        SelectItem::Column { name, .. } => input
            .column(name)
            .map_or(Ok(0), |cell| add(clone_bound(cell, controls, 0)?, 104)),
        SelectItem::Expr { expr, .. } => estimate(expr, input, controls, 0),
        SelectItem::Function { function, .. } => {
            function.expressions().try_fold(0, |bytes, expr| {
                add(bytes, estimate(expr, input, controls, 1)?)
            })
        }
        SelectItem::WindowFunction { .. } => Ok(0),
    }
}

fn estimate(
    expr: &Expr,
    input: &Input<'_>,
    controls: &QueryExecutionControls,
    depth: usize,
) -> Result<usize, CassieError> {
    controls.check_interruption()?;
    if let Expr::Column(name) = expr {
        if let Some(cell) = input.column(name) {
            return clone_bound(cell, controls, depth);
        }
    }
    let mut bytes = 0;
    expr.try_visit_children(|child| {
        bytes = add(bytes, estimate(child, input, controls, depth + 1)?)?;
        Ok::<_, CassieError>(())
    })?;
    Ok(bytes)
}

fn clone_bound(
    expr: &Expr,
    controls: &QueryExecutionControls,
    depth: usize,
) -> Result<usize, CassieError> {
    controls.check_interruption()?;
    if depth > 128 {
        return Err(CassieError::ResourceLimit(
            "constant-source expression nesting exceeds 128 levels".into(),
        ));
    }
    let mut bytes = 4 * size_of::<Expr>();
    match expr {
        Expr::StringLiteral(text) | Expr::Column(text) => {
            bytes = add(bytes, mul(text.capacity(), 4)?)?;
        }
        Expr::Cast {
            data_type,
            source_type,
            ..
        } => {
            bytes = add(bytes, mul(data_type_clone_bytes(data_type)?, 4)?)?;
            if let Some(source) = source_type {
                bytes = add(bytes, mul(data_type_clone_bytes(source)?, 4)?)?;
            }
        }
        Expr::RowCompare(_) => bytes = add(bytes, 4 * size_of::<crate::sql::ast::RowComparison>())?,
        _ => {}
    }
    expr.try_visit_children(|child| {
        bytes = add(bytes, clone_bound(child, controls, depth + 1)?)?;
        Ok::<_, CassieError>(())
    })?;
    Ok(bytes)
}

fn rewrite(
    expr: &mut Expr,
    input: &Input<'_>,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    controls.check_interruption()?;
    if let Expr::Column(name) = expr {
        if let Some(cell) = input.column(name) {
            *expr = cell.clone();
            return Ok(());
        }
    }
    expr.try_visit_children_mut(|child| rewrite(child, input, controls))
}
