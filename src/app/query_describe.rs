use std::collections::HashMap;

use super::{Cassie, CassieError, PlanCacheProvenance};

impl Cassie {
    pub(crate) fn describe_parsed_statement(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
    ) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        self.describe_parsed_statement_with_parameter_oids(parsed, sql_fingerprint, &[])
    }

    pub(crate) fn describe_parsed_statement_with_parameter_oids(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        parameter_type_oids: &[i32],
    ) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        if matches!(
            parsed.statement,
            crate::sql::ast::QueryStatement::Explain(_)
        ) {
            return Ok(vec![crate::executor::ColumnMeta::text("QUERY PLAN")]);
        }
        if matches!(
            parsed.statement,
            crate::sql::ast::QueryStatement::Transaction(_)
        ) {
            return Ok(Vec::new());
        }

        let controls = self.runtime.query_controls(std::time::Instant::now());
        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }

        let cache_key = matches!(parsed.statement, crate::sql::ast::QueryStatement::Select(_))
            .then(|| {
                self.plan_cache_key_from_fingerprint(
                    sql_fingerprint,
                    Vec::new(),
                    crate::runtime::ExecutionMode::DescribeQuery,
                    None,
                    &[crate::catalog::DEFAULT_SCHEMA.to_string()],
                )
            });
        let (physical, provenance) = if let Some(key) = cache_key.clone() {
            self.resolve_physical_plan(parsed, key, None, Some(&controls))?
        } else {
            (
                self.compile_physical_plan(parsed, None, Some(&controls))?,
                PlanCacheProvenance::Compiled,
            )
        };

        let user_functions = if crate::executor::plan_needs_user_functions(&physical.logical) {
            self.catalog
                .list_functions()
                .into_iter()
                .map(|metadata| (metadata.name.to_ascii_lowercase(), metadata))
                .collect::<HashMap<String, _>>()
        } else {
            HashMap::new()
        };
        let collection_schema = self.catalog.get_schema(&physical.logical.collection);

        if let Some(command) = physical.logical.command.as_ref() {
            let returning = match command {
                crate::planner::logical::LogicalCommand::Insert(statement) => {
                    Some(statement.returning.as_slice())
                }
                crate::planner::logical::LogicalCommand::Update(statement) => {
                    Some(statement.returning.as_slice())
                }
                crate::planner::logical::LogicalCommand::Delete(statement) => {
                    Some(statement.returning.as_slice())
                }
                _ => None,
            };
            if let Some(returning) = returning {
                return Ok(crate::executor::columns_from_projection(
                    returning,
                    collection_schema.as_ref(),
                    &user_functions,
                ));
            }
            return Ok(Vec::new());
        }

        if let Some(key) = cache_key.as_ref() {
            self.observe_query_plan_usage(key, &physical, &provenance)?;
        }

        Ok(
            crate::executor::aggregate::columns_from_projection_with_parameter_oids(
                &physical.logical.projection,
                collection_schema.as_ref(),
                &user_functions,
                parameter_type_oids,
            ),
        )
    }
}
