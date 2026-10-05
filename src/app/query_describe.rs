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
            false,
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
            false,
        )
    }

    pub(crate) fn describe_pgwire_parsed_statement_for_session(
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
            true,
        )
    }

    fn describe_parsed_statement_in_session(
        &self,
        session: Option<&super::CassieSession>,
        parsed: crate::sql::ast::ParsedStatement,
        sql_fingerprint: u64,
        parameter_type_oids: &[i32],
        wire_output: bool,
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
                    super::query_parameters::parameter_shape_for_oids(parameter_type_oids),
                    crate::runtime::ExecutionMode::DescribeQuery,
                    database,
                    &search_path,
                )
            });
        let (physical, provenance) = if let Some(key) = cache_key.clone() {
            self.resolve_physical_plan_with_parameter_oids(
                parsed,
                key,
                session,
                Some(&controls),
                parameter_type_oids,
            )?
        } else {
            (
                self.compile_physical_plan_with_parameter_oids(
                    parsed,
                    session,
                    Some(&controls),
                    parameter_type_oids,
                )?,
                PlanCacheProvenance::Compiled,
            )
        };

        crate::sql::binder::validate_coalesce_plan(
            &physical.logical,
            &self.catalog,
            &self.binding_context_for_session(session),
            parameter_type_oids,
            false,
        )?;
        crate::sql::binder::validate_boolean_parameter_plan(
            &physical.logical,
            &self.catalog,
            &self.binding_context_for_session(session),
            parameter_type_oids,
        )?;

        let columns = if wire_output {
            self.pgwire_columns_for_plan(
                &physical.logical,
                session,
                parameter_type_oids,
                &controls,
            )?
        } else {
            self.columns_for_plan(&physical.logical, session, parameter_type_oids)
        };
        if let Some(key) = cache_key.as_ref() {
            self.observe_query_plan_usage(key, &physical, &provenance)?;
        }

        Ok(columns)
    }

    pub(super) fn columns_for_plan(
        &self,
        logical: &crate::planner::logical::LogicalPlan,
        session: Option<&super::CassieSession>,
        parameter_type_oids: &[i32],
    ) -> Vec<crate::executor::ColumnMeta> {
        let user_functions = if crate::executor::plan_needs_user_functions(logical) {
            self.user_functions_for_session(session)
        } else {
            HashMap::new()
        };
        let collection_schema = self.describe_collection_schema(logical, &user_functions);
        if let Some(columns) =
            Self::describe_command_columns(logical, collection_schema.as_ref(), &user_functions)
        {
            return columns;
        }
        let wildcard_fields = crate::executor::aggregate::wildcard_fields_for_plan(
            &self.catalog,
            logical,
            &user_functions,
        );
        crate::executor::aggregate::columns_from_projection_with_wildcard(
            &logical.projection,
            collection_schema.as_ref(),
            wildcard_fields.as_deref(),
            &user_functions,
            parameter_type_oids,
        )
    }

    fn describe_command_columns(
        logical: &crate::planner::logical::LogicalPlan,
        collection_schema: Option<&crate::catalog::CollectionSchema>,
        user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    ) -> Option<Vec<crate::executor::ColumnMeta>> {
        let returning = match logical.command.as_ref()? {
            crate::planner::logical::LogicalCommand::Insert(statement) => &statement.returning,
            crate::planner::logical::LogicalCommand::Update(statement) => &statement.returning,
            crate::planner::logical::LogicalCommand::Delete(statement) => &statement.returning,
            crate::planner::logical::LogicalCommand::Show(statement) => {
                return Some(crate::executor::show_result_columns(statement));
            }
            _ => return Some(Vec::new()),
        };
        if returning.is_empty() {
            return Some(Vec::new());
        }
        Some(crate::executor::columns_from_projection(
            returning,
            collection_schema,
            user_functions,
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
        .or_else(|| {
            crate::sql::binder::joined_source_schema(
                &logical.source,
                &logical.ctes,
                &self.catalog,
                user_functions,
            )
        })
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
