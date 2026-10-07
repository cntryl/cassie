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
        let output = if declared_oids.is_empty() {
            None
        } else {
            let context = self.binding_context_for_session(session);
            crate::sql::binder::infer_plan_output_contract(
                logical,
                &self.catalog,
                &context,
                declared_oids,
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
                &mut columns,
                returning,
                schema_has_id,
            );
        }
        for column in &columns {
            let data_type = DataType::parse_sql(&column.data_type)
                .map_err(|error| CassieError::Execution(format!("invalid result type: {error}")))?;
            if matches!(data_type, DataType::Array(element) if matches!(*element, DataType::Vector(_)))
            {
                return Err(CassieError::Unsupported(
                    "VECTOR array result identities are not supported by pgwire".to_string(),
                ));
            }
        }
        if controls.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        Ok(columns)
    }
}
