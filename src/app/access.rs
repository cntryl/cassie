use super::{CassieError, CassieSession, QueryStatement};
use crate::catalog::{RoleGrantTarget, RoleObjectPrivilege};
use crate::sql::ast::{
    CommonTableExpression, CteQuery, Expr, IdentifierPath, MutationSource, QuerySource,
    ReturningAliases, SelectItem, SelectStatement,
};

impl CassieSession {
    pub(crate) fn authorize_statement(
        &self,
        statement: &QueryStatement,
    ) -> Result<(), CassieError> {
        if self.is_authenticated_read_only()
            && matches!(statement, QueryStatement::Set(setting) if setting.variable.eq_ignore_ascii_case("role"))
        {
            return Err(CassieError::Unsupported(
                "SET ROLE is not supported for authenticated sessions".to_owned(),
            ));
        }
        Ok(())
    }
}

impl super::Cassie {
    pub(crate) fn authorize_role_object_access(
        &self,
        session: &CassieSession,
        statement: &QueryStatement,
    ) -> Result<(), CassieError> {
        if !session.is_network_authenticated() {
            return Ok(());
        }
        let role = self.lookup_role(&session.user)?.or_else(|| {
            session
                .user
                .eq_ignore_ascii_case(&self.auth_user)
                .then(|| crate::catalog::RoleMeta::bootstrap_admin(&self.auth_user, None))
        });
        let Some(role) = role else {
            return Err(CassieError::InsufficientPrivilege);
        };
        if role.is_admin {
            return Ok(());
        }
        let context = self.binding_context_for_session(Some(session));
        self.authorize_non_admin_statement(session, &context, statement)
    }

    fn authorize_non_admin_statement(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        statement: &QueryStatement,
    ) -> Result<(), CassieError> {
        match statement {
            QueryStatement::Explain(explain) => {
                self.authorize_non_admin_statement(session, context, &explain.statement.statement)
            }
            QueryStatement::Select(select) => self.authorize_select(session, context, select),
            QueryStatement::Insert(insert) => self.authorize_insert(session, context, insert),
            QueryStatement::Update(update) => self.authorize_update(session, context, update),
            QueryStatement::Delete(delete) => self.authorize_delete(session, context, delete),
            QueryStatement::Copy(copy) => self.require_table_privilege(
                session,
                context,
                &copy.table,
                RoleObjectPrivilege::Insert,
            ),
            QueryStatement::Merge(merge) => self.authorize_merge(session, context, merge),
            QueryStatement::Show(_)
            | QueryStatement::Set(_)
            | QueryStatement::Transaction(_)
            | QueryStatement::SessionObject(_) => Ok(()),
            _ => Err(CassieError::InsufficientPrivilege),
        }
    }

    fn authorize_insert(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        insert: &crate::sql::ast::InsertStatement,
    ) -> Result<(), CassieError> {
        self.authorize_ctes(session, context, &insert.ctes)?;
        let target = insert.table.to_string();
        let target_read = self.target_row_read_context(
            context,
            &insert.table,
            insert.alias.as_deref(),
            insert.returning_aliases.as_ref(),
            &["excluded"],
        )?;
        let mut target_select_required = false;
        self.require_table_privilege(session, context, &target, RoleObjectPrivilege::Insert)?;
        if let crate::sql::ast::InsertSource::Select(select) = &insert.source {
            self.authorize_select(session, context, select)?;
        }
        if let Some(conflict) = &insert.on_conflict {
            if let Some(predicate) = &conflict.target_predicate {
                self.authorize_expression(session, context, predicate)?;
            }
            for expression in &conflict.target_expressions {
                self.authorize_expression(session, context, expression)?;
            }
            if matches!(
                &conflict.action,
                crate::sql::ast::InsertConflictAction::DoUpdate { .. }
            ) {
                self.require_table_privilege(
                    session,
                    context,
                    &target,
                    RoleObjectPrivilege::Update,
                )?;
            }
            target_select_required |= !conflict.target_fields.is_empty()
                || conflict.constraint.is_some()
                || conflict
                    .target_expressions
                    .iter()
                    .any(|expression| target_read.expression_reads_target(expression, false))
                || conflict
                    .target_predicate
                    .as_ref()
                    .is_some_and(|expression| {
                        target_read.expression_reads_target(expression, false)
                    })
                || conflict.update_row_subqueries.iter().any(|row_subquery| {
                    target_read.parsed_statement_reads_target(&row_subquery.query)
                });
            if let crate::sql::ast::InsertConflictAction::DoUpdate {
                assignments,
                filter,
            } = &conflict.action
            {
                for (_, expression) in assignments {
                    self.authorize_expression(session, context, expression)?;
                    target_select_required |=
                        target_read.expression_reads_target(expression, false);
                }
                if let Some(filter) = filter {
                    self.authorize_expression(session, context, filter)?;
                    target_select_required |= target_read.expression_reads_target(filter, false);
                }
            }
            for row_subquery in &conflict.update_row_subqueries {
                self.authorize_non_admin_statement(
                    session,
                    context,
                    &row_subquery.query.statement,
                )?;
            }
        }
        for expression in insert_expressions(insert) {
            self.authorize_expression(session, context, expression)?;
        }
        target_select_required |= target_read.select_items_read_target(&insert.returning, false);
        if target_select_required {
            self.require_table_privilege(session, context, &target, RoleObjectPrivilege::Select)?;
        }
        self.authorize_returning(session, context, &insert.returning)
    }

    fn authorize_update(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        update: &crate::sql::ast::UpdateStatement,
    ) -> Result<(), CassieError> {
        self.authorize_ctes(session, context, &update.ctes)?;
        let target = update.table.to_string();
        let target_read = self.target_row_read_context(
            context,
            &update.table,
            update.alias.as_deref(),
            update.returning_aliases.as_ref(),
            &[],
        )?;
        self.require_table_privilege(session, context, &target, RoleObjectPrivilege::Update)?;
        let expressions = update_expressions(update);
        let target_select_required = !update.array_targets.is_empty()
            || expressions.iter().any(|expression| {
                target_read.expression_reads_target(expression, update.from.is_some())
            })
            || update
                .from
                .as_ref()
                .is_some_and(|source| target_read.mutation_source_reads_target(source))
            || target_read.select_items_read_target(&update.returning, update.from.is_some())
            || update
                .row_subqueries
                .iter()
                .any(|row_subquery| target_read.parsed_statement_reads_target(&row_subquery.query));
        if target_select_required {
            self.require_table_privilege(session, context, &target, RoleObjectPrivilege::Select)?;
        }
        if let Some(source) = &update.from {
            self.authorize_mutation_source(session, context, source)?;
        }
        for expression in expressions {
            self.authorize_expression(session, context, expression)?;
        }
        self.authorize_returning(session, context, &update.returning)?;
        for row_subquery in &update.row_subqueries {
            self.authorize_non_admin_statement(session, context, &row_subquery.query.statement)?;
        }
        Ok(())
    }

    fn authorize_delete(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        delete: &crate::sql::ast::DeleteStatement,
    ) -> Result<(), CassieError> {
        self.authorize_ctes(session, context, &delete.ctes)?;
        let target = delete.table.to_string();
        let target_read = self.target_row_read_context(
            context,
            &delete.table,
            delete.alias.as_deref(),
            delete.returning_aliases.as_ref(),
            &[],
        )?;
        self.require_table_privilege(session, context, &target, RoleObjectPrivilege::Delete)?;
        let expressions = delete_expressions(delete);
        let target_select_required = expressions.iter().any(|expression| {
            target_read.expression_reads_target(expression, delete.using.is_some())
        }) || delete
            .using
            .as_ref()
            .is_some_and(|source| target_read.mutation_source_reads_target(source))
            || target_read.select_items_read_target(&delete.returning, delete.using.is_some());
        if target_select_required {
            self.require_table_privilege(session, context, &target, RoleObjectPrivilege::Select)?;
        }
        if let Some(source) = &delete.using {
            self.authorize_mutation_source(session, context, source)?;
        }
        for expression in expressions {
            self.authorize_expression(session, context, expression)?;
        }
        self.authorize_returning(session, context, &delete.returning)
    }

    fn authorize_merge(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        merge: &crate::sql::ast::MergeStatement,
    ) -> Result<(), CassieError> {
        self.authorize_ctes(session, context, &merge.ctes)?;
        let target = merge.table.to_string();
        let target_read = self.target_row_read_context(
            context,
            &merge.table,
            merge.alias.as_deref(),
            merge.returning_aliases.as_ref(),
            &[],
        )?;
        self.authorize_query_source(session, context, &merge.source.source)?;
        self.authorize_expression(session, context, &merge.on)?;
        let mut target_select_required = target_read.expression_reads_target(&merge.on, true)
            || target_read.mutation_source_reads_target(&merge.source)
            || target_read.select_items_read_target(&merge.returning, true);
        let mut needs_insert = false;
        let mut needs_update = false;
        let mut needs_delete = false;
        for clause in &merge.clauses {
            if let Some(condition) = &clause.condition {
                target_select_required |= target_read.expression_reads_target(condition, true);
            }
            match &clause.action {
                crate::sql::ast::MergeAction::Insert { .. } => needs_insert = true,
                crate::sql::ast::MergeAction::Update { .. } => needs_update = true,
                crate::sql::ast::MergeAction::Delete => needs_delete = true,
                crate::sql::ast::MergeAction::DoNothing => target_select_required = true,
            }
            match &clause.action {
                crate::sql::ast::MergeAction::Update {
                    assignments,
                    row_subqueries,
                } => {
                    target_select_required |= assignments.iter().any(|(_, expression)| {
                        target_read.expression_reads_target(expression, true)
                    }) || row_subqueries.iter().any(|row_subquery| {
                        target_read.parsed_statement_reads_target(&row_subquery.query)
                    });
                }
                crate::sql::ast::MergeAction::Insert { values, .. } => {
                    target_select_required |= values
                        .iter()
                        .flatten()
                        .any(|expression| target_read.expression_reads_target(expression, true));
                }
                crate::sql::ast::MergeAction::Delete | crate::sql::ast::MergeAction::DoNothing => {}
            }
        }
        if target_select_required {
            self.require_table_privilege(session, context, &target, RoleObjectPrivilege::Select)?;
        }
        for (needed, privilege) in [
            (needs_insert, RoleObjectPrivilege::Insert),
            (needs_update, RoleObjectPrivilege::Update),
            (needs_delete, RoleObjectPrivilege::Delete),
        ] {
            if needed {
                self.require_table_privilege(session, context, &target, privilege)?;
            }
        }
        for clause in &merge.clauses {
            if let Some(condition) = &clause.condition {
                self.authorize_expression(session, context, condition)?;
            }
            match &clause.action {
                crate::sql::ast::MergeAction::Update {
                    assignments,
                    row_subqueries,
                } => {
                    for (_, expression) in assignments {
                        self.authorize_expression(session, context, expression)?;
                    }
                    for row_subquery in row_subqueries {
                        self.authorize_non_admin_statement(
                            session,
                            context,
                            &row_subquery.query.statement,
                        )?;
                    }
                }
                crate::sql::ast::MergeAction::Insert { values, .. } => {
                    for expression in values.iter().flatten() {
                        self.authorize_expression(session, context, expression)?;
                    }
                }
                crate::sql::ast::MergeAction::Delete | crate::sql::ast::MergeAction::DoNothing => {}
            }
        }
        self.authorize_returning(session, context, &merge.returning)
    }

    fn authorize_select(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        select: &SelectStatement,
    ) -> Result<(), CassieError> {
        self.authorize_ctes(session, context, &select.ctes)?;
        self.authorize_query_source(session, context, &select.source)?;
        for item in &select.projection {
            self.authorize_select_item(session, context, item)?;
        }
        for expression in select
            .filter
            .iter()
            .chain(select.distinct_on.iter())
            .chain(select.group_by.iter())
            .chain(select.having.iter())
            .chain(select.order.iter().map(|order| &order.expr))
            .chain(select.limit.iter())
            .chain(select.offset.iter())
        {
            self.authorize_expression(session, context, expression)?;
        }
        if let Some(set) = &select.set {
            self.authorize_select(session, context, &set.right)?;
        }
        Ok(())
    }

    fn authorize_returning(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        items: &[SelectItem],
    ) -> Result<(), CassieError> {
        for item in items {
            self.authorize_select_item(session, context, item)?;
        }
        Ok(())
    }

    fn authorize_select_item(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        item: &SelectItem,
    ) -> Result<(), CassieError> {
        match item {
            SelectItem::Function { function, .. } => {
                for expression in function.expressions() {
                    self.authorize_expression(session, context, expression)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expression in function
                    .args
                    .iter()
                    .chain(function.filter.as_deref())
                    .chain(function.partition_by.iter())
                    .chain(function.order_by.iter().map(|order| &order.expr))
                {
                    self.authorize_expression(session, context, expression)?;
                }
            }
            SelectItem::Expr { expr, .. } => {
                self.authorize_expression(session, context, expr)?;
            }
            SelectItem::Wildcard | SelectItem::Column { .. } => {}
        }
        Ok(())
    }

    fn authorize_ctes(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        ctes: &[CommonTableExpression],
    ) -> Result<(), CassieError> {
        for cte in ctes {
            match &cte.query {
                CteQuery::Simple(query) => {
                    self.authorize_non_admin_statement(session, context, &query.statement)?;
                }
                CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    self.authorize_non_admin_statement(session, context, &base.statement)?;
                    self.authorize_non_admin_statement(session, context, &recursive.statement)?;
                }
            }
        }
        Ok(())
    }

    fn authorize_query_source(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        source: &QuerySource,
    ) -> Result<(), CassieError> {
        match source {
            QuerySource::Collection(name) => self.require_table_privilege(
                session,
                context,
                name.as_ref(),
                RoleObjectPrivilege::Select,
            ),
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.authorize_query_source(session, context, left)?;
                self.authorize_query_source(session, context, right)?;
                self.authorize_expression(session, context, on)
            }
            QuerySource::Aliased { source, .. } => {
                self.authorize_query_source(session, context, source)
            }
            QuerySource::TableSample {
                source,
                rate,
                repeatable,
                ..
            } => {
                self.authorize_query_source(session, context, source)?;
                self.authorize_expression(session, context, rate)?;
                if let Some(repeatable) = repeatable {
                    self.authorize_expression(session, context, repeatable)?;
                }
                Ok(())
            }
            QuerySource::Subquery { select, .. } => self.authorize_select(session, context, select),
            QuerySource::TableFunction { function, .. } => {
                for expression in function.expressions() {
                    self.authorize_expression(session, context, expression)?;
                }
                Ok(())
            }
            QuerySource::Values { rows } => {
                for expression in rows.iter().flatten() {
                    self.authorize_expression(session, context, expression)?;
                }
                Ok(())
            }
            QuerySource::Cte(_) | QuerySource::SingleRow => Ok(()),
        }
    }

    fn authorize_mutation_source(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        source: &MutationSource,
    ) -> Result<(), CassieError> {
        self.authorize_query_source(session, context, &source.source)
    }

    fn authorize_expression(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        expression: &Expr,
    ) -> Result<(), CassieError> {
        if let Some(statement) = expression.subquery_statement() {
            self.authorize_non_admin_statement(session, context, &statement.statement)?;
        }
        let mut result = Ok(());
        expression.for_each_child(|child| {
            if result.is_ok() {
                result = self.authorize_expression(session, context, child);
            }
        });
        result
    }

    fn target_row_read_context(
        &self,
        context: &crate::sql::binder::BindingContext,
        relation: &IdentifierPath,
        alias: Option<&str>,
        returning_aliases: Option<&ReturningAliases>,
        additional_aliases: &[&str],
    ) -> Result<TargetRowReadContext, CassieError> {
        let resolved =
            crate::sql::binder::resolve_relation_name(relation.as_ref(), &self.catalog, context)?;
        let relation_path = IdentifierPath::parse(&resolved).map_err(CassieError::Parse)?;
        let relation_qualifiers = if alias.is_some() {
            Vec::new()
        } else {
            (0..relation_path.components().len())
                .map(|start| {
                    IdentifierPath::from_components(relation_path.components()[start..].to_vec())
                        .into_string()
                })
                .collect()
        };
        let mut aliases = alias.map_or_else(Vec::new, |alias| vec![alias.to_owned()]);
        if let Some(returning_aliases) = returning_aliases {
            aliases.push(returning_aliases.old.clone());
            aliases.push(returning_aliases.new.clone());
        }
        aliases.extend(additional_aliases.iter().map(|alias| (*alias).to_owned()));
        let columns = self
            .catalog
            .get_schema(&resolved)
            .map_or_else(Vec::new, |schema| {
                schema.fields.into_iter().map(|field| field.name).collect()
            });
        Ok(TargetRowReadContext {
            aliases,
            relation_qualifiers,
            columns,
        })
    }

    fn require_table_privilege(
        &self,
        session: &CassieSession,
        context: &crate::sql::binder::BindingContext,
        relation: &str,
        privilege: RoleObjectPrivilege,
    ) -> Result<(), CassieError> {
        let resolved = crate::sql::binder::resolve_relation_name(relation, &self.catalog, context)?;
        let schema_name = crate::catalog::relation_schema_name(&resolved);
        if crate::catalog::is_system_schema(&schema_name) {
            return Ok(());
        }
        let Some(collection) = self.catalog.get_collection(&resolved) else {
            return Err(CassieError::InsufficientPrivilege);
        };
        let database = crate::catalog::relation_database_name(&resolved)
            .or_else(|| session.current_database().map(str::to_owned))
            .unwrap_or_else(|| self.default_database.clone());
        let Some(database_meta) = self.catalog.get_database(&database) else {
            return Err(CassieError::InsufficientPrivilege);
        };
        let Some(schema) = self
            .catalog
            .get_namespace(&crate::catalog::canonical_schema_name(
                &database,
                &schema_name,
            ))
        else {
            return Err(CassieError::InsufficientPrivilege);
        };
        let Some(role) = self.catalog.get_role(&session.user) else {
            return Err(CassieError::InsufficientPrivilege);
        };
        let schema_usage = self.catalog.get_role_object_grant(
            &database_meta.physical_family,
            role.object_id,
            &RoleGrantTarget::Schema(schema.object_id),
        );
        if !schema_usage.is_some_and(|grant| grant.grants(RoleObjectPrivilege::Usage)) {
            return Err(CassieError::InsufficientPrivilege);
        }
        let table_grant = self.catalog.get_role_object_grant(
            &database_meta.physical_family,
            role.object_id,
            &RoleGrantTarget::Table(collection.storage_id),
        );
        if table_grant.is_some_and(|grant| grant.grants(privilege)) {
            Ok(())
        } else {
            Err(CassieError::InsufficientPrivilege)
        }
    }
}

struct TargetRowReadContext {
    aliases: Vec<String>,
    relation_qualifiers: Vec<String>,
    columns: Vec<String>,
}

impl TargetRowReadContext {
    fn expression_reads_target(&self, expression: &Expr, has_source: bool) -> bool {
        if let Some(statement) = expression.subquery_statement() {
            return self.parsed_statement_reads_target(statement);
        }
        if let Expr::Column(name) = expression {
            return self.column_reference_reads_target(name, has_source);
        }
        let mut reads_target = false;
        expression.for_each_child(|child| {
            if !reads_target && self.expression_reads_target(child, has_source) {
                reads_target = true;
            }
        });
        reads_target
    }

    fn parsed_statement_reads_target(&self, statement: &crate::sql::ast::ParsedStatement) -> bool {
        match &statement.statement {
            crate::sql::ast::QueryStatement::Select(select) => {
                self.select_statement_reads_target(select)
            }
            crate::sql::ast::QueryStatement::Explain(explain) => {
                self.parsed_statement_reads_target(&explain.statement)
            }
            _ => true,
        }
    }

    fn select_statement_reads_target(&self, statement: &SelectStatement) -> bool {
        let has_source = !matches!(&statement.source, QuerySource::SingleRow);
        self.subquery_select_items_read_target(&statement.projection, has_source)
            || statement
                .distinct_on
                .iter()
                .any(|expression| self.expression_reads_target(expression, false))
            || statement
                .filter
                .as_ref()
                .is_some_and(|expression| self.expression_reads_target(expression, false))
            || statement
                .group_by
                .iter()
                .any(|expression| self.expression_reads_target(expression, false))
            || statement
                .having
                .as_ref()
                .is_some_and(|expression| self.expression_reads_target(expression, false))
            || statement
                .order
                .iter()
                .any(|order| self.expression_reads_target(&order.expr, false))
            || statement
                .limit
                .as_ref()
                .is_some_and(|expression| self.expression_reads_target(expression, false))
            || statement
                .offset
                .as_ref()
                .is_some_and(|expression| self.expression_reads_target(expression, false))
            || self.query_source_reads_target(&statement.source)
            || statement.ctes.iter().any(|cte| match &cte.query {
                CteQuery::Simple(query) => self.parsed_statement_reads_target(query),
                CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    self.parsed_statement_reads_target(base)
                        || self.parsed_statement_reads_target(recursive)
                }
            })
            || statement
                .set
                .as_ref()
                .is_some_and(|set| self.select_statement_reads_target(&set.right))
    }

    fn subquery_select_items_read_target(&self, items: &[SelectItem], has_source: bool) -> bool {
        items.iter().any(|item| match item {
            SelectItem::Wildcard => !has_source,
            SelectItem::Column { name, .. } => self.column_reference_reads_target(name, false),
            SelectItem::Expr { expr, .. } => self.expression_reads_target(expr, false),
            SelectItem::Function { function, .. } => function
                .expressions()
                .any(|expression| self.expression_reads_target(expression, false)),
            SelectItem::WindowFunction { function, .. } => function
                .args
                .iter()
                .chain(function.filter.as_deref())
                .chain(function.partition_by.iter())
                .chain(function.order_by.iter().map(|order| &order.expr))
                .any(|expression| self.expression_reads_target(expression, false)),
        })
    }

    fn select_items_read_target(&self, items: &[SelectItem], has_source: bool) -> bool {
        items.iter().any(|item| match item {
            SelectItem::Wildcard => true,
            SelectItem::Column { name, .. } => self.column_reference_reads_target(name, has_source),
            SelectItem::Expr { expr, .. } => self.expression_reads_target(expr, has_source),
            SelectItem::Function { function, .. } => function
                .expressions()
                .any(|expression| self.expression_reads_target(expression, has_source)),
            SelectItem::WindowFunction { function, .. } => function
                .args
                .iter()
                .chain(function.filter.as_deref())
                .chain(function.partition_by.iter())
                .chain(function.order_by.iter().map(|order| &order.expr))
                .any(|expression| self.expression_reads_target(expression, has_source)),
        })
    }

    fn mutation_source_reads_target(&self, source: &MutationSource) -> bool {
        source
            .query
            .as_deref()
            .is_some_and(|query| self.select_statement_reads_target(query))
            || self.query_source_reads_target(&source.source)
    }

    fn query_source_reads_target(&self, source: &QuerySource) -> bool {
        match source {
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.query_source_reads_target(left)
                    || self.query_source_reads_target(right)
                    || self.expression_reads_target(on, true)
            }
            QuerySource::Aliased { source, .. } => self.query_source_reads_target(source),
            QuerySource::TableSample {
                source,
                rate,
                repeatable,
                ..
            } => {
                self.query_source_reads_target(source)
                    || self.expression_reads_target(rate, true)
                    || repeatable
                        .as_ref()
                        .is_some_and(|expression| self.expression_reads_target(expression, true))
            }
            QuerySource::Subquery { select, .. } => self.select_statement_reads_target(select),
            QuerySource::TableFunction { function, .. } => function
                .expressions()
                .any(|expression| self.expression_reads_target(expression, true)),
            QuerySource::Values { rows } => rows
                .iter()
                .flatten()
                .any(|expression| self.expression_reads_target(expression, true)),
            QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
        }
    }

    fn column_reference_reads_target(&self, name: &str, has_source: bool) -> bool {
        if name == "*" {
            return true;
        }
        let Ok(path) = crate::sql::ColumnIdentifierPath::parse_sql(name) else {
            return false;
        };
        if !path.is_qualified() {
            return !has_source
                || self.columns.iter().any(|column| {
                    crate::sql::ColumnIdentifierPath::matches_stored_field(
                        &path.field_lookup_key(),
                        column,
                    )
                });
        }
        let qualifiers = path.namespace_prefix_keys();
        self.aliases.iter().any(|alias| {
            qualifiers
                .iter()
                .any(|qualifier| alias.eq_ignore_ascii_case(qualifier))
        }) || self.relation_qualifiers.iter().any(|relation| {
            qualifiers
                .iter()
                .any(|qualifier| relation.eq_ignore_ascii_case(qualifier))
        })
    }
}

fn insert_expressions(statement: &crate::sql::ast::InsertStatement) -> Vec<&Expr> {
    let mut expressions = Vec::new();
    if let crate::sql::ast::InsertSource::Values(rows) = &statement.source {
        expressions.extend(rows.iter().flatten());
    }
    expressions
}

fn update_expressions(statement: &crate::sql::ast::UpdateStatement) -> Vec<&Expr> {
    statement
        .assignments
        .iter()
        .map(|(_, expression)| expression)
        .chain(statement.filter.iter())
        .collect()
}

fn delete_expressions(statement: &crate::sql::ast::DeleteStatement) -> Vec<&Expr> {
    statement.filter.iter().collect()
}
