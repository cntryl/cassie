//! Finite wire output metadata and numeric input provenance.
//!
//! The caller supplies an already bound/normalized logical plan. Ordinary
//! validation remains the caller's responsibility. This module does not bind,
//! compile, cache, execute, or change the original `ParameterDescription` OIDs.

#[path = "numeric_outputs/domain.rs"]
mod domain;
#[path = "numeric_outputs/expressions.rs"]
mod expressions;
#[path = "numeric_outputs/scope.rs"]
mod scope;

use super::{inference, BindingContext, CassieError, Catalog};
use crate::planner::logical::{LogicalCommand, LogicalPlan};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    BinaryOp, CommonTableExpression, CteQuery, Expr, FunctionCall, QuerySource, QueryStatement,
    SelectItem, SelectStatement, SetOperator,
};
use crate::types::{DataType, FieldSchema, Schema};
use domain::{OutputField, OutputType};
use std::collections::HashMap;

type OutputFields = Vec<OutputField>;
type CteScope = HashMap<String, OutputFields>;

/// Facts are kept positionally; names are presentation, not origin identity.
pub(crate) struct OutputContract {
    pub(crate) schema: Schema,
    pub(crate) unresolved_numeric: Vec<bool>,
}

impl OutputContract {
    pub(crate) fn require_supported_numeric_output(&self) -> Result<(), CassieError> {
        if self.unresolved_numeric.iter().any(|unresolved| *unresolved) {
            return Err(CassieError::Unsupported(
                "decoder-only numeric output requires an existing-type CAST".to_string(),
            ));
        }
        Ok(())
    }
}

struct Analyzer<'a> {
    catalog: &'a Catalog,
    functions: HashMap<String, crate::catalog::FunctionMeta>,
    declared_oids: &'a [i32],
    controls: &'a QueryExecutionControls,
}

impl Analyzer<'_> {
    fn check_controls(&self) -> Result<(), CassieError> {
        if self.controls.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
        if self.controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        Ok(())
    }
}

/// Returns `None` for command families whose existing metadata path is fixed.
/// A command with no RETURNING produces an empty, supported contract.
pub(crate) fn infer_plan_output_contract(
    logical: &LogicalPlan,
    catalog: &Catalog,
    context: &BindingContext,
    declared_oids: &[i32],
    controls: &QueryExecutionControls,
) -> Result<Option<OutputContract>, CassieError> {
    let analyzer = Analyzer {
        catalog,
        functions: crate::catalog::function_resolution::functions_for_scope(
            &catalog.list_functions(),
            &context.database,
            &context.search_path,
            context.scopes_database_objects(),
        ),
        declared_oids,
        controls,
    };
    analyzer.check_controls()?;
    let fields = if let Some(command) = &logical.command {
        let (table, returning) = match command {
            LogicalCommand::Insert(statement) => (&statement.table, &statement.returning),
            LogicalCommand::Update(statement) => (&statement.table, &statement.returning),
            LogicalCommand::Delete(statement) => (&statement.table, &statement.returning),
            _ => return Ok(None),
        };
        let source = QuerySource::Collection(table.clone());
        let source = analyzer.source(&source, &CteScope::new(), None)?;
        analyzer.project(returning, &source.output, &source.lookup)?
    } else {
        // Mirrors plan_inspection::logical_plan_from_select in reverse.
        // `recursive` is carried by each CteQuery, not this root flag.
        let select = SelectStatement {
            source: logical.source.clone(),
            ctes: logical.ctes.clone(),
            recursive: false,
            distinct: logical.distinct,
            distinct_on: logical.distinct_on.clone(),
            projection: logical.projection.clone(),
            filter: logical.filter.clone(),
            group_by: logical.group_by.clone(),
            having: logical.having.clone(),
            order: logical.order.clone(),
            limit: logical.limit.clone(),
            offset: logical.offset.clone(),
            set: logical.set.clone(),
        };
        analyzer.select(&select, &CteScope::new(), None)?
    };
    let schema = Schema {
        fields: fields.iter().map(OutputField::schema_field).collect(),
    };
    Ok(Some(OutputContract {
        schema,
        unresolved_numeric: fields
            .iter()
            .map(|field| field.value.unresolved_numeric)
            .collect(),
    }))
}
