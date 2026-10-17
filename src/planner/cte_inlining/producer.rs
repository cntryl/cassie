//! Preserve positional CTE output aliases when replacing a reference.
use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{CommonTableExpression, QuerySource, SelectItem, SelectStatement};

pub(super) fn copy(
    cte: &CommonTableExpression,
    select: &SelectStatement,
    controls: &QueryExecutionControls,
    memory: &mut QueryMemoryReservation,
) -> Result<Box<SelectStatement>, CassieError> {
    controls.check_interruption()?;
    let aliases = !cte.aliases.is_empty() && cte.aliases[0] != "*";
    let wildcard = aliases
        && select
            .projection
            .iter()
            .any(|item| matches!(item, SelectItem::Wildcard));
    let width = if wildcard {
        let QuerySource::Values { rows } = &select.source else {
            return Err(CassieError::Unsupported(
                "NOT MATERIALIZED wildcard relation aliases remain pending under #801".into(),
            ));
        };
        if !matches!(select.projection.as_slice(), [SelectItem::Wildcard]) {
            return Err(CassieError::Unsupported(
                "NOT MATERIALIZED mixed wildcard aliases remain pending under #801".into(),
            ));
        }
        rows.first().map_or(0, Vec::len)
    } else {
        0
    };
    let extra = mul(width, add(size_of::<SelectItem>(), 256)?)?;
    memory.try_grow(mul(
        add(
            crate::sql::parse_analysis::select_clone_bytes(select, controls)?,
            extra,
        )?,
        4,
    )?)?;
    let mut select = Box::new(select.clone());
    if wildcard {
        let mut projection = Vec::with_capacity(width);
        for index in 0..width {
            controls.check_interruption()?;
            let name = format!("column{}", index + 1);
            projection.push(SelectItem::Column {
                alias: Some(
                    cte.aliases
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| name.clone()),
                ),
                name,
            });
        }
        select.projection = projection;
    } else if aliases {
        for (item, alias) in select.projection.iter_mut().zip(&cte.aliases) {
            controls.check_interruption()?;
            match item {
                SelectItem::Column { alias: output, .. }
                | SelectItem::Expr { alias: output, .. }
                | SelectItem::Function { alias: output, .. }
                | SelectItem::WindowFunction { alias: output, .. } => *output = Some(alias.clone()),
                SelectItem::Wildcard => unreachable!("wildcard handled separately"),
            }
        }
    }
    Ok(select)
}
