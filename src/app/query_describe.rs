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
        self.describe_parsed_statement_in_session(
            None,
            parsed,
            sql_fingerprint,
            parameter_type_oids,
        )
    }

    pub(crate) fn describe_parsed_statement_for_session(
        &self,
        session: &super::CassieSession,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        parameter_type_oids: &[i32],
    ) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        self.describe_parsed_statement_in_session(
            Some(session),
            parsed,
            sql_fingerprint,
            parameter_type_oids,
        )
    }

    fn describe_parsed_statement_in_session(
        &self,
        session: Option<&super::CassieSession>,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        parameter_type_oids: &[i32],
    ) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        if let Some(session) = session {
            self.ensure_session_database_access(session)?;
        }
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

        let search_path = session.map_or_else(
            || vec![crate::catalog::DEFAULT_SCHEMA.to_string()],
            super::CassieSession::search_path,
        );
        let database = session
            .and_then(super::CassieSession::current_database)
            .map(str::to_string);
        let cache_key = matches!(parsed.statement, crate::sql::ast::QueryStatement::Select(_))
            .then(|| {
                self.plan_cache_key_from_fingerprint(
                    sql_fingerprint,
                    Vec::new(),
                    crate::runtime::ExecutionMode::DescribeQuery,
                    database,
                    &search_path,
                )
            });
        let (physical, provenance) = if let Some(key) = cache_key.clone() {
            self.resolve_physical_plan(parsed, key, session, Some(&controls))?
        } else {
            (
                self.compile_physical_plan(parsed, session, Some(&controls))?,
                PlanCacheProvenance::Compiled,
            )
        };

        let user_functions = if crate::executor::plan_needs_user_functions(&physical.logical) {
            self.user_functions_for_session(session)
        } else {
            HashMap::new()
        };
        let collection_schema = self.describe_collection_schema(&physical.logical, &user_functions);

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
                crate::planner::logical::LogicalCommand::Show(statement) => {
                    return Ok(crate::executor::show_result_columns(statement));
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

        let wildcard_fields = crate::executor::aggregate::wildcard_fields_for_plan(
            &self.catalog,
            &physical.logical,
            &user_functions,
        );
        let columns = crate::executor::aggregate::columns_from_projection_with_wildcard(
            &physical.logical.projection,
            collection_schema.as_ref(),
            wildcard_fields.as_deref(),
            &user_functions,
            parameter_type_oids,
        );
        Ok(crate::executor::unify_set_result_columns(
            &self.catalog,
            &physical.logical,
            &user_functions,
            parameter_type_oids,
            columns,
        ))
    }

    fn describe_collection_schema(
        &self,
        logical: &crate::planner::logical::LogicalPlan,
        user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    ) -> Option<crate::catalog::CollectionSchema> {
        crate::sql::binder::derived_source_schema(
            &logical.source,
            &logical.ctes,
            &self.catalog,
            user_functions,
        )
        .or_else(|| self.catalog.get_schema(&logical.collection))
        .or_else(|| crate::catalog::CollectionSchema::virtual_view(&logical.collection))
        .or_else(|| {
            crate::sql::binder::cte_collection_schema_with_functions(
                &logical.ctes,
                &logical.collection,
                &self.catalog,
                user_functions,
            )
        })
    }
}
