//! Private typed transport for the selected scan/filter/projection contract.

use std::mem::size_of;
use std::sync::Arc;

use super::QueryError;
use crate::app::CassieError;
use crate::executor::retained_memory::{add, data_type_clone_bytes, mul};
use crate::runtime::accounted::Accounted;
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};

pub(crate) mod capability;
mod column;
pub(crate) mod join;
mod operations;
pub(crate) mod relational;
pub(crate) mod relational_diagnostics;
pub(crate) mod row_bridge;
pub(crate) mod scalar;
#[cfg(test)]
mod tests;
mod validity;

pub(crate) use column::{Cell, Column};

type Schema = Vec<(String, DataType)>;

#[derive(Debug, Clone)]
pub(crate) struct TypedBatch {
    schema: Arc<Accounted<Schema>>,
    columns: Arc<Accounted<Vec<Column>>>,
    domain: usize,
    selection: Option<Arc<Accounted<Vec<usize>>>>,
}

impl TypedBatch {
    pub(crate) fn from_columns(
        controls: &QueryExecutionControls,
        schema: &[(String, DataType)],
        values: &[Vec<Value>],
        domain: usize,
        selection: Option<&[usize]>,
    ) -> Result<Self, QueryError> {
        if schema.len() != values.len() || values.iter().any(|values| values.len() != domain) {
            return Err(invalid("schema, column count and domain disagree"));
        }
        check_selection(domain, selection)?;
        let _memory = controls.reserve_query_memory(mul(values.len(), size_of::<Column>())?)?;
        let mut columns = Vec::with_capacity(values.len());
        for ((_, data_type), values) in schema.iter().zip(values) {
            columns.push(Column::from_values(controls, data_type, values)?);
        }
        Self::from_views(controls, schema, &columns, domain, selection)
    }

    pub(crate) fn from_views(
        controls: &QueryExecutionControls,
        schema: &[(String, DataType)],
        columns: &[Column],
        domain: usize,
        selection: Option<&[usize]>,
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        check_selection(domain, selection)?;
        if schema.len() != columns.len()
            || schema.iter().zip(columns).any(|((_, data_type), column)| {
                column.len() != domain || column.data_type() != data_type
            })
        {
            return Err(invalid("schema, column type and domain disagree"));
        }
        let schema_bytes = schema.iter().try_fold(
            owner_bytes::<Schema>(mul(schema.len(), size_of::<(String, DataType)>())?)?,
            |bytes, (name, data_type)| {
                add(bytes, add(name.len(), data_type_clone_bytes(data_type)?)?)
            },
        )?;
        let schema = Arc::new(Accounted::try_new(controls, schema_bytes, || {
            schema.to_vec()
        })?);
        let columns = Arc::new(Accounted::try_new(
            controls,
            owner_bytes::<Vec<Column>>(mul(columns.len(), size_of::<Column>())?)?,
            || columns.to_vec(),
        )?);
        let selection = selection
            .map(|selection| shared_map(controls, selection))
            .transpose()?;
        check_controls(controls)?;
        Ok(Self {
            schema,
            columns,
            domain,
            selection,
        })
    }

    pub(crate) fn len(&self) -> usize {
        self.selection
            .as_ref()
            .map_or(self.domain, |selection| selection.get().len())
    }

    pub(crate) fn schema(&self) -> &[(String, DataType)] {
        self.schema.get()
    }

    pub(crate) fn slice(
        &self,
        controls: &QueryExecutionControls,
        start: usize,
        len: usize,
    ) -> Result<Self, QueryError> {
        if start.checked_add(len).is_none_or(|end| end > self.len()) {
            return Err(invalid("batch slice is outside logical domain"));
        }
        if let Some(selection) = &self.selection {
            return Self::from_views(
                controls,
                self.schema(),
                self.columns.get(),
                self.domain,
                Some(&selection.get()[start..start + len]),
            );
        }
        let _memory =
            controls.reserve_query_memory(mul(self.columns.get().len(), size_of::<Column>())?)?;
        let columns = self
            .columns
            .get()
            .iter()
            .map(|column| column.slice(controls, start, len))
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_views(controls, self.schema(), &columns, len, None)
    }

    pub(crate) fn position(&self, lane: usize) -> Result<usize, QueryError> {
        if lane >= self.len() {
            return Err(invalid("logical lane is outside batch"));
        }
        Ok(self
            .selection
            .as_ref()
            .map_or(lane, |selection| selection.get()[lane]))
    }

    pub(crate) fn cell(&self, column: usize, lane: usize) -> Result<Cell<'_>, QueryError> {
        self.columns
            .get()
            .get(column)
            .ok_or_else(|| invalid("column is outside schema"))?
            .cell(self.position(lane)?)
    }

    #[cfg(test)]
    pub(crate) fn value(&self, column: usize, lane: usize) -> Result<Value, QueryError> {
        self.cell(column, lane).map(Cell::to_owned)
    }
}

fn check_selection(domain: usize, selection: Option<&[usize]>) -> Result<(), QueryError> {
    if selection.is_some_and(|selection| selection.iter().any(|position| *position >= domain)) {
        return Err(invalid("selection is outside base domain"));
    }
    Ok(())
}

pub(super) fn shared_map(
    controls: &QueryExecutionControls,
    positions: &[usize],
) -> Result<Arc<Accounted<Vec<usize>>>, QueryError> {
    Ok(Arc::new(Accounted::try_new(
        controls,
        owner_bytes::<Vec<usize>>(mul(positions.len(), size_of::<usize>())?)?,
        || positions.to_vec(),
    )?))
}

pub(super) fn owner_bytes<T>(heap: usize) -> Result<usize, CassieError> {
    add(
        add(size_of::<Accounted<T>>(), 2 * size_of::<usize>())?,
        heap,
    )
}

pub(super) fn invalid(message: &str) -> QueryError {
    QueryError::General(format!("invalid typed batch: {message}"))
}

pub(super) fn check_controls(controls: &QueryExecutionControls) -> Result<(), QueryError> {
    if controls.is_cancelled() {
        Err(CassieError::QueryCancelled.into())
    } else if controls.is_timed_out() {
        Err(CassieError::DeadlineExceeded.into())
    } else {
        Ok(())
    }
}

impl TypedBatch {
    pub(crate) fn column_index(&self, name: &str) -> Option<usize> {
        self.schema()
            .iter()
            .position(|(column, _)| column == name)
            .or_else(|| {
                let reference = crate::sql::ColumnIdentifierPath::parse(name).ok()?;
                let candidates = reference.row_lookup_candidates();
                self.schema()
                    .iter()
                    .position(|(column, _)| candidates.iter().any(|candidate| candidate == column))
            })
    }
}

pub(super) fn admitted<T>(
    value: T,
    memory: crate::runtime::QueryMemoryReservation,
) -> Accounted<T> {
    Accounted::from_admitted(value, memory)
}
