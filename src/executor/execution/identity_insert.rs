//! Identity column rules for `INSERT`: `GENERATED ALWAYS` rejects explicit
//! values unless `OVERRIDING SYSTEM VALUE`, and `OVERRIDING USER VALUE`
//! discards explicit identity values so the sequence supplies them.
use super::{Cassie, FieldMeta, QueryError};
use crate::catalog::{IdentityGeneration, SequenceError};
use crate::sql::ast::{InsertOverriding, InsertStatement};

pub(super) fn apply_identity_policy<T>(
    cassie: &Cassie,
    statement: &InsertStatement,
    fields: Vec<FieldMeta>,
    rows: Vec<Vec<T>>,
    is_default: impl Fn(&T) -> bool,
) -> Result<(Vec<FieldMeta>, Vec<Vec<T>>), QueryError> {
    let constraints = cassie.catalog.get_constraints(statement.table.as_ref());
    let identity = |field: &FieldMeta| {
        constraints
            .iter()
            .find(|constraint| constraint.field == field.name)
            .and_then(|constraint| constraint.identity)
    };
    let mut discarded = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        match (identity(field), statement.overriding) {
            (Some(_), Some(InsertOverriding::User)) => discarded.push(index),
            (Some(IdentityGeneration::Always), None)
                if rows.is_empty()
                    || rows
                        .iter()
                        .any(|row| row.get(index).is_none_or(|cell| !is_default(cell))) =>
            {
                return Err(QueryError::Cassie(
                    SequenceError {
                        sqlstate: "428C9",
                        message: format!(
                            "cannot insert a non-DEFAULT value into column \"{}\"",
                            field.name
                        ),
                    }
                    .into(),
                ));
            }
            _ => {}
        }
    }
    if discarded.is_empty() {
        return Ok((fields, rows));
    }
    let keep = |index: &usize| !discarded.contains(index);
    let fields = fields
        .into_iter()
        .enumerate()
        .filter(|(index, _)| keep(index))
        .map(|(_, field)| field)
        .collect();
    let rows = rows
        .into_iter()
        .map(|row| {
            row.into_iter()
                .enumerate()
                .filter(|(index, _)| keep(index))
                .map(|(_, value)| value)
                .collect()
        })
        .collect();
    Ok((fields, rows))
}
