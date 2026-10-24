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
        if is_create_table_as(&parsed) {
            return Ok(Vec::new());
        }
        if let crate::sql::ast::QueryStatement::SessionObject(statement) = &parsed.statement {
            return self.describe_session_object(session, statement, wire_output);
        }
        let (scoped_cassie, can_use_shared_plan_cache) = self.catalog_scope_for_session(session);
        let controls = scoped_cassie
            .runtime
            .query_controls(std::time::Instant::now());
        let admitted = super::query_parameters::admit_parameter_types(
            &scoped_cassie,
            session,
            &parsed,
            crate::sql::PrivateParameterTypeInput::Wire(parameter_type_oids),
            &controls,
        )?;
        let parameter_type_oids = admitted
            .as_ref()
            .map_or(parameter_type_oids, |types| types.types());
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

        if controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }

        let cache_key = describe_plan_cache_key(
            &scoped_cassie,
            can_use_shared_plan_cache,
            &parsed,
            sql_fingerprint,
            parameter_type_oids,
            session,
        );
        let (physical, provenance) = if let Some(key) = cache_key.clone() {
            scoped_cassie.resolve_physical_plan_with_parameter_oids(
                parsed,
                key,
                session,
                Some(&controls),
                parameter_type_oids,
            )?
        } else {
            (
                scoped_cassie.compile_physical_plan_with_parameter_oids(
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
            &scoped_cassie.catalog,
            &scoped_cassie.binding_context_for_session(session),
            parameter_type_oids,
            false,
        )?;
        crate::sql::binder::validate_boolean_parameter_plan(
            &physical.logical,
            &scoped_cassie.catalog,
            &scoped_cassie.binding_context_for_session(session),
            parameter_type_oids,
        )?;

        let columns = if wire_output {
            scoped_cassie.pgwire_columns_for_plan(
                &physical.logical,
                session,
                parameter_type_oids,
                &controls,
            )?
        } else {
            scoped_cassie.columns_for_plan(&physical.logical, session, parameter_type_oids)
        };
        if let Some(key) = cache_key.as_ref() {
            scoped_cassie.observe_query_plan_usage(key, &physical, &provenance)?;
        }

        Ok(columns)
    }

    fn catalog_scope_for_session(&self, session: Option<&super::CassieSession>) -> (Self, bool) {
        let catalog_overlay = session.and_then(super::CassieSession::transaction_catalog_overlay);
        let mut scoped_cassie = self.clone();
        if let Some(catalog) = catalog_overlay.as_ref() {
            scoped_cassie.catalog = catalog.clone();
        }
        (scoped_cassie, catalog_overlay.is_none())
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
        if let Some(columns) = self.describe_joined_command_columns(logical, &user_functions) {
            return columns;
        }
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
        let mut columns = crate::executor::aggregate::columns_from_projection_with_wildcard(
            &logical.projection,
            collection_schema.as_ref(),
            wildcard_fields.as_deref(),
            &user_functions,
            parameter_type_oids,
        );
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
        columns
    }

    /// `RETURNING` columns of `UPDATE ... FROM`, `DELETE ... USING` and `MERGE`,
    /// which resolve against the target joined to its source.
    fn describe_joined_command_columns(
        &self,
        logical: &crate::planner::logical::LogicalPlan,
        user_functions: &HashMap<String, crate::catalog::FunctionMeta>,
    ) -> Option<Vec<crate::executor::ColumnMeta>> {
        use crate::planner::logical::LogicalCommand;
        let (table, alias, source, aliases, merge, returning) = match logical.command.as_ref()? {
            LogicalCommand::Update(statement) if crate::executor::update_is_joined(statement) => (
                &statement.table,
                statement.alias.as_deref(),
                statement.from.as_ref(),
                statement.returning_aliases.as_ref(),
                false,
                &statement.returning,
            ),
            LogicalCommand::Delete(statement) if crate::executor::delete_is_joined(statement) => (
                &statement.table,
                statement.alias.as_deref(),
                statement.using.as_ref(),
                statement.returning_aliases.as_ref(),
                false,
                &statement.returning,
            ),
            LogicalCommand::Merge(statement) => (
                &statement.table,
                statement.alias.as_deref(),
                Some(&statement.source),
                statement.returning_aliases.as_ref(),
                true,
                &statement.returning,
            ),
            _ => return None,
        };
        if returning.is_empty() {
            return Some(Vec::new());
        }
        let schema = self.catalog.get_schema(table.as_str())?;
        let qualifier = alias.map_or_else(|| crate::catalog::local_name(table), str::to_string);
        let joined = crate::executor::joined_returning_schema(
            &schema,
            &qualifier,
            source.map_or(&[][..], |source| source.fields.as_slice()),
            aliases,
            merge,
        );
        Some(crate::executor::columns_from_projection(
            returning,
            Some(&joined),
            user_functions,
        ))
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
            crate::planner::logical::LogicalCommand::VerifyProjection(_) => {
                return Some(Self::fixed_result_columns(
                    crate::executor::projection_verification_column_definitions(),
                ));
            }
            crate::planner::logical::LogicalCommand::DiffProjection(_) => {
                return Some(Self::fixed_result_columns(
                    crate::executor::diff_projection_column_definitions(),
                ));
            }
            crate::planner::logical::LogicalCommand::CompareProjection(_) => {
                return Some(Self::fixed_result_columns(
                    crate::executor::compare_projection_column_definitions(),
                ));
            }
            crate::planner::logical::LogicalCommand::PlanRepairProjection(_)
            | crate::planner::logical::LogicalCommand::RepairProjection(_) => {
                return Some(Self::fixed_result_columns(
                    crate::executor::repair_projection_column_definitions(),
                ));
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

    fn fixed_result_columns(
        definitions: impl IntoIterator<Item = (&'static str, crate::types::DataType)>,
    ) -> Vec<crate::executor::ColumnMeta> {
        definitions
            .into_iter()
            .map(|(name, data_type)| crate::executor::ColumnMeta::from_data_type(name, &data_type))
            .collect()
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
        .or_else(|| {
            self.catalog
                .get_materialized_projection(&logical.collection)
                .and_then(|projection| projection.materialized)
                .map(|materialized| {
                    crate::catalog::CollectionSchema::from_type_schema(
                        logical.collection.clone(),
                        &materialized.output_schema,
                    )
                })
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

fn is_create_table_as(parsed: &crate::sql::ast::ParsedStatement) -> bool {
    matches!(
        &parsed.statement,
        crate::sql::ast::QueryStatement::CreateTable(statement)
            if statement.as_query.is_some()
    )
}

fn describe_plan_cache_key(
    cassie: &Cassie,
    can_use_shared_plan_cache: bool,
    parsed: &crate::sql::ast::ParsedStatement,
    sql_fingerprint: u64,
    parameter_type_oids: &[i32],
    session: Option<&super::CassieSession>,
) -> Option<crate::runtime::PlanCacheKey> {
    if !can_use_shared_plan_cache
        || !matches!(
            &parsed.statement,
            crate::sql::ast::QueryStatement::Select(_)
        )
    {
        return None;
    }
    let search_path = session.map_or_else(
        || vec![crate::catalog::DEFAULT_SCHEMA.to_string()],
        super::CassieSession::search_path,
    );
    let database = session
        .and_then(super::CassieSession::current_database)
        .map(str::to_string);
    Some(cassie.plan_cache_key_from_fingerprint(
        sql_fingerprint,
        parameter_type_oids,
        crate::runtime::ExecutionMode::DescribeQuery,
        database,
        &search_path,
    ))
}
