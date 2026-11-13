//! The finite pgwire output profile over an already compiled logical plan.

use super::{Cassie, CassieError, CassieSession, ColumnMeta, QueryExecutionControls};
use crate::planner::logical::{LogicalCommand, LogicalPlan};
use crate::types::DataType;

impl Cassie {
    pub(super) fn bound_statement_output_columns(
        &self,
        logical: &LogicalPlan,
        session: &CassieSession,
        declared_oids: &[i32],
        controls: &QueryExecutionControls,
        wire_output: bool,
    ) -> Result<Option<Vec<ColumnMeta>>, CassieError> {
        let has_sql_output_contract = matches!(
            logical.command.as_ref(),
            None | Some(
                LogicalCommand::Insert(_) | LogicalCommand::Update(_) | LogicalCommand::Delete(_)
            )
        );
        if wire_output && has_sql_output_contract {
            Ok(Some(self.pgwire_columns_for_plan(
                logical,
                Some(session),
                declared_oids,
                controls,
            )?))
        } else {
            Ok(None)
        }
    }

    pub(super) fn pgwire_columns_for_plan(
        &self,
        logical: &LogicalPlan,
        session: Option<&CassieSession>,
        declared_oids: &[i32],
        controls: &QueryExecutionControls,
    ) -> Result<Vec<ColumnMeta>, CassieError> {
        let output = if declared_oids.is_empty() && logical.set.is_none() {
            None
        } else {
            let context = self.binding_context_for_session(session);
            // Inference sees the active database's vector OID as its private marker.
            let mut inference_oids = declared_oids.to_vec();
            crate::sql::binder::normalize_parameter_type_oids_for_database(
                &mut inference_oids,
                &self.catalog,
                &context.database,
            );
            crate::sql::binder::infer_plan_output_contract(
                logical,
                &self.catalog,
                &context,
                &inference_oids,
                controls,
            )?
        };
        let mut columns = if let Some(output) = output {
            output.require_supported_numeric_output()?;
            output
                .schema
                .fields
                .into_iter()
                .map(|field| ColumnMeta::from_data_type(field.name, &field.data_type))
                .collect()
        } else {
            self.columns_for_plan(logical, session, declared_oids)
        };
        let _ = crate::executor::aggregate::apply_percentile_array_oids_for_query(
            &logical.source,
            &logical.ctes,
            &logical.projection,
            &mut columns,
        );
        crate::executor::aggregate::apply_recursive_cte_record_oids_for_query(
            &logical.source,
            &logical.ctes,
            &logical.projection,
            &mut columns,
        );
        crate::executor::aggregate::apply_void_function_oids(&logical.projection, &mut columns);
        crate::executor::aggregate::apply_pg_type_kind_oid(
            &logical.source,
            &logical.projection,
            &mut columns,
        );
        crate::executor::aggregate::apply_pg_attribute_identity_oid(
            &logical.source,
            &logical.projection,
            &mut columns,
        );
        self.normalize_returning_identity_columns(logical, &mut columns);
        let database = session
            .and_then(CassieSession::current_database)
            .unwrap_or(&self.default_database);
        for column in &mut columns {
            // Record columns carry their own flag (table row types have no
            // composite catalog entry); their display name is not a
            // parseable scalar type name.
            let is_record_column = column.is_record
                || u32::try_from(column.type_oid).is_ok_and(|oid| {
                    oid == crate::types::composite::RECORD_OID
                        || self.catalog.get_composite_type_by_oid(oid).is_some()
                });
            if is_record_column {
                continue;
            }
            let data_type = DataType::parse_sql(&column.data_type)
                .map_err(|error| CassieError::Execution(format!("invalid result type: {error}")))?;
            if matches!(data_type, DataType::Vector(_)) {
                column.type_oid = crate::catalog::object_identity::type_oid_for_database(
                    &self.catalog,
                    database,
                    &data_type,
                )
                .ok_or_else(|| {
                    crate::app::DiagnosticError::new("42704", "type \"vector\" does not exist")
                })?;
            }
            if matches!(data_type, DataType::Array(element) if matches!(*element, DataType::Vector(_)))
            {
                return Err(CassieError::Unsupported(
                    "VECTOR array result identities are not supported by pgwire".to_string(),
                ));
            }
            if (34_000..44_000).contains(&column.type_oid) {
                return Err(CassieError::Unsupported(
                    "private Cassie array result OIDs are unsupported by pgwire".to_string(),
                ));
            }
        }
        controls.check_interruption()?;
        Ok(columns)
    }

    /// Aligns DML RETURNING identity columns with the target table's declared id.
    fn normalize_returning_identity_columns(
        &self,
        logical: &LogicalPlan,
        columns: &mut [ColumnMeta],
    ) {
        let returning = match logical.command.as_ref() {
            Some(LogicalCommand::Insert(statement)) => {
                Some((&statement.table, &statement.returning))
            }
            Some(LogicalCommand::Update(statement)) => {
                Some((&statement.table, &statement.returning))
            }
            Some(LogicalCommand::Delete(statement)) => {
                Some((&statement.table, &statement.returning))
            }
            _ => None,
        };
        if let Some((table, returning)) = returning {
            let schema_has_id = self
                .catalog
                .get_schema(table.as_str())
                .is_some_and(|schema| schema.declares_id());
            crate::executor::aggregate::normalize_dml_returning_identity_columns(
                columns,
                returning,
                schema_has_id,
            );
        }
    }
}
