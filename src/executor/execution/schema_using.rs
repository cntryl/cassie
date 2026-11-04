//! Controlled preparation for the existing row-authoritative USING rewrite.
use std::mem::size_of;

use super::{ensure_row_store_alter_supported, refresh_table_cardinality_stats};
use crate::app::{Cassie, CassieError, CassieSession};
use crate::catalog::CollectionSchema;
use crate::executor::filter::{self, EvaluationMemory};
use crate::executor::retained_memory::{add, data_type_clone_bytes, mul, value_clone_bytes};
use crate::executor::{scan, QueryError};
use crate::midge::adapter::{DocumentRef, RowDecode};
use crate::runtime::accounted::{json, AccountedVec};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{AlterTableOperation, Expr};
use crate::types::{DataType, Value};

pub(super) struct Context<'a> {
    pub(super) cassie: &'a Cassie,
    pub(super) session: Option<&'a CassieSession>,
    pub(super) controls: &'a QueryExecutionControls,
    pub(super) is_column_store: bool,
}

impl Context<'_> {
    pub(super) fn preflight(
        &self,
        table: &str,
        operations: &[AlterTableOperation],
    ) -> Result<(), QueryError> {
        for operation in operations {
            if let AlterTableOperation::AlterColumnType { field, using, .. } = operation {
                self.evaluate(table, field, using)?;
            }
        }
        Ok(())
    }

    fn evaluate(
        &self,
        table: &str,
        field: &str,
        expression: &Expr,
    ) -> Result<AccountedVec<(String, serde_json::Value)>, QueryError> {
        self.controls.check_interruption()?;
        let (schema, _schema_memory) = self
            .cassie
            .catalog
            .clone_schema_with_controls(table, self.controls)?;
        let schema = schema.ok_or_else(|| CassieError::CollectionNotFound(table.to_owned()))?;
        let functions = std::collections::HashMap::new();
        let mut prepared = AccountedVec::try_new(self.controls)?;
        self.cassie
            .midge
            .scan_rows_until(table, RowDecode::Full, self.controls, |document| {
                let _row_memory = self
                    .controls
                    .reserve_query_memory(row_bytes(&document, &schema)?)?;
                let row = scan::document_to_row(&document, Some(&schema));
                let row = scan::hydrate_character_arrays(row, self.controls)?;
                let _expression_memory = self.controls.reserve_query_memory(
                    crate::executor::execution::source::source_set_function::argument_scratch(
                        expression,
                        &[],
                        &row,
                    )?,
                )?;
                let memory = EvaluationMemory::new(self.controls)?;
                let value = filter::evaluate_source_argument(
                    &row,
                    expression,
                    &[],
                    &functions,
                    self.session,
                    self.controls,
                    &memory,
                )?;
                let bytes = mul(
                    add(
                        add(
                            json::retained_bytes(&document.payload)?,
                            json_value_bytes(&value, self.controls)?,
                        )?,
                        add(document.id.capacity(), add(field.len(), 1024)?)?,
                    )?,
                    4,
                )?;
                prepared.try_push_with_result(bytes, || {
                    let value = match value {
                        Value::Array(array) => array.to_storage_json(),
                        value => crate::types::json::try_value_to_json(value).map_err(|error| {
                            CassieError::from(QueryError::General(error.into()))
                        })?,
                    };
                    let mut payload = document.payload;
                    let object = payload.as_object_mut().ok_or_else(|| {
                        CassieError::from(QueryError::General(format!(
                            "row in '{table}' is not an object"
                        )))
                    })?;
                    object.insert(field.to_owned(), value);
                    Ok((document.id, payload))
                })?;
                #[cfg(test)]
                AFTER_PREPARED.with(|slot| {
                    if let Some(cancellation) = slot.borrow_mut().take() {
                        cancellation.cancel();
                    }
                });
                Ok::<_, QueryError>(true)
            })?;
        self.controls.check_interruption()?;
        Ok(prepared)
    }

    pub(super) fn apply(
        &self,
        table: &str,
        field: &str,
        data_type: &DataType,
        using: &Expr,
    ) -> Result<(), QueryError> {
        ensure_row_store_alter_supported(self.is_column_store, "ALTER COLUMN TYPE")?;
        let prepared = self.evaluate(table, field, using)?;
        let (prepared, prepared_memory) = prepared.into_parts();
        let mut writes = AccountedVec::try_new(self.controls)?;
        for (id, payload) in prepared {
            writes.try_push_with(0, || (Some(id), payload))?;
        }
        let (writes, write_memory) = writes.into_parts();
        // Complete conversion and admission before the existing mutation sequence starts.
        #[cfg(test)]
        {
            AT_REWRITE.with(|slot| {
                slot.set((
                    prepared_memory.bytes(),
                    write_memory.bytes(),
                    self.controls.current_query_memory_bytes(),
                ))
            });
            BEFORE_REWRITE.with(|slot| {
                if let Some(cancellation) = slot.borrow_mut().take() {
                    cancellation.cancel();
                }
            });
        }
        self.controls.check_interruption()?;
        for (id, _) in &writes {
            self.cassie
                .midge
                .delete_document(table, id.as_deref().expect("prepared rewrite ID"))?;
        }
        self.cassie
            .midge
            .alter_collection_column_type(table, field, data_type.clone())?;
        if !writes.is_empty() {
            self.cassie.midge.put_documents(table, writes)?;
        }
        self.cassie
            .catalog
            .set_collection_field_type(table, field, data_type.clone());
        self.cassie.bump_schema_epoch_and_invalidate_query_cache()?;
        drop(write_memory);
        drop(prepared_memory);
        refresh_table_cardinality_stats(self.cassie, table)
    }
}

fn row_bytes(document: &DocumentRef, schema: &CollectionSchema) -> Result<usize, CassieError> {
    let mut bytes = add(1024, mul(json::retained_bytes(&document.payload)?, 8)?)?;
    bytes = add(bytes, mul(document.id.capacity(), 8)?)?;
    bytes = add(bytes, mul(schema.fields.len(), 512)?)?;
    for field in &schema.fields {
        bytes = add(
            bytes,
            add(
                mul(field.name.len(), 128)?,
                mul(data_type_clone_bytes(&field.data_type)?, 8)?,
            )?,
        )?;
    }
    Ok(bytes)
}

fn json_value_bytes(
    value: &Value,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    controls.check_interruption()?;
    let bytes = match value {
        Value::Typed(typed) => crate::executor::native_result_text_bound(
            typed,
            &filter::temporal::session_of(Some(controls)),
        )?,
        Value::Json(value) => json::retained_bytes(value)?,
        value => value_clone_bytes(value)?,
    };
    add(mul(bytes, 8)?, size_of::<serde_json::Value>())
}

#[cfg(test)]
thread_local! { static AFTER_PREPARED: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
thread_local! {
    static AT_REWRITE: std::cell::Cell<(usize,usize,usize)> = const { std::cell::Cell::new((0,0,0)) };
    static BEFORE_REWRITE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
#[path = "schema_using_tests.rs"]
mod tests;
