//! Stream schema-backed source rows through the existing controlled Midge transport.
use super::{BatchRow, QueryError, SourceExecutionEnv};
use crate::executor::retained_memory::{add, mul};
use crate::runtime::QueryMemoryReservation;
use std::mem::size_of;

pub(super) struct Cursor<'a> {
    stream: crate::executor::scan::ProjectedScanStream<'a>,
    qualifier: Option<String>,
    _memory: QueryMemoryReservation,
}

impl<'a> Cursor<'a> {
    pub(super) fn open(
        env: &SourceExecutionEnv<'a>,
        name: &str,
        qualify: bool,
        traversal: crate::midge::adapter::RowTraversal,
    ) -> Result<Option<Self>, QueryError> {
        env.controls.check_interruption()?;
        if super::virtual_views::schema(name).is_some()
            || env.cassie.catalog.get_view(name).is_some()
            || env
                .cassie
                .catalog
                .get_materialized_projection(name)
                .is_some()
            || !super::scan::uses_controlled_row_scan(env.cassie, name)
        {
            return Ok(None);
        }
        let (schema, schema_memory) = env
            .cassie
            .catalog
            .clone_schema_with_controls(name, env.controls)?;
        let Some(schema) = schema else {
            return Ok(None);
        };
        // The transport clones its schema and projected field directory. Keep
        // those execution copies admitted independently from the catalog clone.
        let mut bytes = add(
            schema_memory
                .as_ref()
                .map_or(0, QueryMemoryReservation::bytes),
            add(size_of::<Self>(), mul(name.len(), 2)?)?,
        )?;
        bytes = add(bytes, mul(schema.fields.len() + 1, size_of::<String>())?)?;
        for field in &schema.fields {
            bytes = add(bytes, mul(field.name.len(), 128)?)?;
        }
        let memory = env.controls.reserve_query_memory(add(bytes, 512)?)?;
        let mut fields = Vec::with_capacity(schema.fields.len() + 1);
        fields.push(crate::types::row_identity::ROW_IDENTITY_COLUMN.to_owned());
        for field in &schema.fields {
            fields.push(crate::sql::ColumnIdentifierPath::stored_field_key(
                &field.name,
            ));
        }
        let Some(stream) = crate::executor::scan::projected_scan_stream_with_traversal(
            env.cassie,
            env.session,
            name,
            &fields,
            None,
            traversal,
            env.controls,
        )?
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            stream,
            qualifier: qualify.then(|| name.to_owned()),
            _memory: memory,
        }))
    }

    pub(super) fn next(
        &mut self,
        env: &SourceExecutionEnv<'_>,
    ) -> Result<Option<BatchRow>, QueryError> {
        env.controls.check_interruption()?;
        let Some(row) = self.stream.next_retained_row()? else {
            return Ok(None);
        };
        #[cfg(test)]
        super::source_set_function::probe::after_collection_row();
        if let Some(qualifier) = &self.qualifier {
            let batches =
                super::source_qualification::qualify_owned(env, vec![vec![row]], qualifier)?;
            Ok(batches.into_iter().flatten().next())
        } else {
            Ok(Some(row))
        }
    }
}
