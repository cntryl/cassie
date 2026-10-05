use super::{BindingContext, CassieError, Catalog, Expr, SelectItem, SelectStatement};
use crate::types::{DataType, Schema};

pub(super) struct ResultTypes {
    schema: Schema,
    cte_schemas: std::collections::HashMap<String, Schema>,
    parameter_types: Vec<i32>,
    contextual_parameters: bool,
    functions: std::collections::HashMap<String, crate::catalog::FunctionMeta>,
}

impl ResultTypes {
    pub(super) fn expression_type(&self, expr: &Expr) -> Option<DataType> {
        if self.has_unresolved_qualified_column(expr) {
            return None;
        }
        super::inference::infer_expr_type(
            expr,
            &self.schema,
            &self.functions,
            &self.parameter_types,
        )
    }

    pub(super) fn projection_type(&self, item: &SelectItem) -> Option<DataType> {
        super::inference::infer_projection_schema_with_parameters(
            std::slice::from_ref(item),
            &self.schema,
            &self.functions,
            &self.parameter_types,
        )
        .fields
        .into_iter()
        .next()
        .map(|field| field.data_type)
    }

    fn has_unresolved_qualified_column(&self, expr: &Expr) -> bool {
        if let Expr::Column(name) = expr {
            if let Ok(column) = crate::sql::ColumnIdentifierPath::parse(name) {
                if column.is_qualified() {
                    return !self.schema.fields.iter().any(|field| {
                        crate::sql::ColumnIdentifierPath::parse(&field.name).is_ok_and(
                            |candidate| {
                                candidate.is_qualified()
                                    && candidate.lookup_key() == column.lookup_key()
                            },
                        )
                    });
                }
            }
        }
        expr.try_visit_children(|child| {
            if self.has_unresolved_qualified_column(child) {
                Err(())
            } else {
                Ok(())
            }
        })
        .is_err()
    }

    pub(super) fn parameter_types(&self) -> &[i32] {
        &self.parameter_types
    }

    pub(super) fn cte_schemas(&self) -> &std::collections::HashMap<String, Schema> {
        &self.cte_schemas
    }

    pub(super) fn with_outer_fields(mut self, outer: &Self) -> Self {
        // Inner fields lead the unqualified lookup; qualified outer fields
        // remain available to correlated expressions.
        self.schema
            .fields
            .extend(outer.schema.fields.iter().cloned());
        self
    }

    pub(super) fn with_excluded_fields(mut self, schema: &super::CollectionSchema) -> Self {
        let excluded = schema
            .fields
            .iter()
            .map(|field| crate::types::FieldSchema {
                name: format!(
                    "excluded.{}",
                    crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key()
                ),
                data_type: field.data_type.clone(),
                nullable: true,
            })
            .collect::<Vec<_>>();
        self.schema.fields.extend(excluded);
        self
    }

    pub(super) fn for_source(
        source: &crate::sql::ast::QuerySource,
        ctes: &[crate::sql::ast::CommonTableExpression],
        catalog: &Catalog,
        context: &BindingContext,
    ) -> Result<Self, CassieError> {
        Self::for_scope(
            source,
            ctes,
            catalog,
            context,
            &std::collections::HashMap::new(),
        )
    }

    pub(super) fn for_source_with_parameters(
        source: &crate::sql::ast::QuerySource,
        ctes: &[crate::sql::ast::CommonTableExpression],
        catalog: &Catalog,
        context: &BindingContext,
        parameter_types: &[i32],
    ) -> Result<Self, CassieError> {
        Self::for_scope_with_parameters(
            source,
            ctes,
            catalog,
            context,
            &std::collections::HashMap::new(),
            parameter_types,
        )
    }

    pub(super) fn for_scope(
        source: &crate::sql::ast::QuerySource,
        ctes: &[crate::sql::ast::CommonTableExpression],
        catalog: &Catalog,
        context: &BindingContext,
        outer: &std::collections::HashMap<String, Schema>,
    ) -> Result<Self, CassieError> {
        Self::for_scope_with_parameters(source, ctes, catalog, context, outer, &[])
    }

    pub(super) fn for_scope_with_parameters(
        source: &crate::sql::ast::QuerySource,
        ctes: &[crate::sql::ast::CommonTableExpression],
        catalog: &Catalog,
        context: &BindingContext,
        outer: &std::collections::HashMap<String, Schema>,
        parameter_types: &[i32],
    ) -> Result<Self, CassieError> {
        let functions = crate::catalog::function_resolution::functions_for_scope(
            &catalog.list_functions(),
            &context.database,
            &context.search_path,
            context.scopes_database_objects(),
        );
        let mut schemas = outer.clone();
        let infer = (|| {
            for cte in ctes {
                let schema = super::inference::infer_cte_schema_with_parameters(
                    cte,
                    catalog,
                    &schemas,
                    &functions,
                    parameter_types,
                )?;
                schemas.insert(cte.name.to_ascii_lowercase(), schema);
            }
            super::inference::infer_source_schema_with_parameters(
                source,
                catalog,
                &schemas,
                &functions,
                true,
                parameter_types,
            )
        })();
        let schema = match infer {
            Ok(mut schema) => {
                super::inference::append_source_qualifiers(source, catalog, &mut schema)?;
                schema
            }
            Err(CassieError::CollectionNotFound(name))
                if source_references_cte(source, &name)
                    || ctes.iter().any(|cte| cte_references_name(cte, &name)) =>
            {
                // A CTE body is bound before its enclosing CTE schemas exist.
                // The enclosing select repeats validation with those schemas.
                Schema { fields: Vec::new() }
            }
            Err(error) => return Err(error),
        };
        Ok(Self {
            schema,
            cte_schemas: schemas,
            functions,
            parameter_types: parameter_types.to_vec(),
            contextual_parameters: false,
        })
    }

    pub(super) fn item(&self, item: &SelectItem) -> Result<(), CassieError> {
        match item {
            SelectItem::Function { function, .. } => {
                self.expression(&Expr::Function(function.clone()))
            }
            SelectItem::Expr { expr, .. } => self.expression(expr),
            SelectItem::WindowFunction { function, .. } => {
                for expr in function
                    .args
                    .iter()
                    .chain(&function.partition_by)
                    .chain(function.order_by.iter().map(|order| &order.expr))
                {
                    self.expression(expr)?;
                }
                Ok(())
            }
            SelectItem::Wildcard | SelectItem::Column { .. } => Ok(()),
        }
    }

    pub(super) fn expression(&self, expr: &Expr) -> Result<(), CassieError> {
        expr.try_visit_children(|child| self.expression(child))?;
        if let Expr::Function(function) = expr {
            if crate::sql::functions::function(&function.name).is_some_and(|metadata| {
                matches!(
                    metadata.return_type,
                    crate::sql::functions::FunctionReturnType::FirstNonNullArgument
                )
            }) {
                let value_context = function
                    .args
                    .iter()
                    .filter(|arg| !matches!(arg, Expr::Param(_)))
                    .filter_map(|arg| {
                        super::inference::infer_expr_type(
                            arg,
                            &self.schema,
                            &self.functions,
                            &self.parameter_types,
                        )
                    })
                    .find(|data_type| {
                        matches!(
                            data_type,
                            DataType::Uuid
                                | DataType::Bytea
                                | DataType::Date
                                | DataType::Time
                                | DataType::Timestamp
                                | DataType::Array(_)
                        )
                    });
                let mut result = DataType::Null;
                for arg in &function.args {
                    if let Some(mut arg_type) = super::inference::infer_expr_type(
                        arg,
                        &self.schema,
                        &self.functions,
                        &self.parameter_types,
                    ) {
                        // Embedded UUID/BYTEA/temporal values use strings, and
                        // arrays use JSON. Wire description retains exact OIDs.
                        if self.contextual_parameters && matches!(arg, Expr::Param(_)) {
                            if let Some(context) = &value_context {
                                let compatible_representation = matches!(
                                    (&arg_type, context),
                                    (DataType::Json, DataType::Array(_))
                                        | (
                                            DataType::Text,
                                            DataType::Uuid
                                                | DataType::Bytea
                                                | DataType::Date
                                                | DataType::Time
                                                | DataType::Timestamp
                                        )
                                );
                                if compatible_representation {
                                    arg_type = context.clone();
                                }
                            }
                        }
                        result = super::inference::common_case_type(result, arg_type).ok_or_else(
                            || CassieError::Planner("incompatible COALESCE result types".into()),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

pub(super) fn validate_select(
    select: &SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    validate_parameter_select(
        select,
        catalog,
        context,
        &[],
        false,
        &std::collections::HashMap::new(),
    )
}

fn validate_source(
    types: &ResultTypes,
    source: &crate::sql::ast::QuerySource,
) -> Result<(), CassieError> {
    match source {
        crate::sql::ast::QuerySource::Join {
            left, right, on, ..
        } => {
            types.expression(on)?;
            validate_source(types, left)?;
            validate_source(types, right)
        }
        crate::sql::ast::QuerySource::TableFunction { function, .. } => {
            for arg in &function.args {
                types.expression(arg)?;
            }
            Ok(())
        }
        _ => Ok(()), // Derived queries are validated in their own binding scope.
    }
}

pub(crate) fn validate_plan(
    plan: &crate::planner::logical::LogicalPlan,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
    contextual_parameters: bool,
) -> Result<(), CassieError> {
    validate_plan_in_scope(
        plan,
        catalog,
        context,
        parameter_types,
        contextual_parameters,
        &std::collections::HashMap::new(),
    )
}

fn validate_plan_in_scope(
    plan: &crate::planner::logical::LogicalPlan,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
    contextual_parameters: bool,
    outer: &std::collections::HashMap<String, Schema>,
) -> Result<(), CassieError> {
    if !plan_contains_coalesce(plan) {
        return Ok(());
    }
    let mut types = ResultTypes::for_scope_with_parameters(
        &plan.source,
        &plan.ctes,
        catalog,
        context,
        outer,
        parameter_types,
    )?;
    types.contextual_parameters = contextual_parameters;
    for item in &plan.projection {
        types.item(item)?;
    }
    for expr in plan
        .filter
        .iter()
        .chain(&plan.distinct_on)
        .chain(&plan.group_by)
        .chain(&plan.having)
        .chain(plan.order.iter().map(|order| &order.expr))
    {
        types.expression(expr)?;
    }
    if let Some(command) = &plan.command {
        let (table, returning) = match command {
            crate::planner::logical::LogicalCommand::Insert(statement) => {
                (&statement.table, &statement.returning)
            }
            crate::planner::logical::LogicalCommand::Update(statement) => {
                (&statement.table, &statement.returning)
            }
            crate::planner::logical::LogicalCommand::Delete(statement) => {
                (&statement.table, &statement.returning)
            }
            _ => return Ok(()),
        };
        types.schema = ResultTypes::for_source_with_parameters(
            &crate::sql::ast::QuerySource::Collection(table.clone()),
            &[],
            catalog,
            context,
            parameter_types,
        )?
        .schema;
        for item in returning {
            types.item(item)?;
        }
    }
    validate_source(&types, &plan.source)?;
    validate_nested_source(
        &plan.source,
        catalog,
        context,
        parameter_types,
        contextual_parameters,
        &types.cte_schemas,
    )?;
    if let Some(set) = &plan.set {
        validate_parameter_select(
            &set.right,
            catalog,
            context,
            parameter_types,
            contextual_parameters,
            &types.cte_schemas,
        )?;
    }
    validate_parameter_ctes(
        &plan.ctes,
        catalog,
        context,
        parameter_types,
        contextual_parameters,
        &types.cte_schemas,
    )
}

fn validate_parameter_ctes(
    ctes: &[crate::sql::ast::CommonTableExpression],
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
    contextual_parameters: bool,
    outer: &std::collections::HashMap<String, Schema>,
) -> Result<(), CassieError> {
    for cte in ctes {
        match &cte.query {
            crate::sql::ast::CteQuery::Simple(statement) => validate_parameter_statement(
                statement,
                catalog,
                context,
                parameter_types,
                contextual_parameters,
                outer,
            )?,
            crate::sql::ast::CteQuery::Recursive {
                base, recursive, ..
            } => {
                validate_parameter_statement(
                    base,
                    catalog,
                    context,
                    parameter_types,
                    contextual_parameters,
                    outer,
                )?;
                validate_parameter_statement(
                    recursive,
                    catalog,
                    context,
                    parameter_types,
                    contextual_parameters,
                    outer,
                )?;
            }
        }
    }
    Ok(())
}

fn validate_parameter_statement(
    statement: &crate::sql::ast::ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
    contextual_parameters: bool,
    outer: &std::collections::HashMap<String, Schema>,
) -> Result<(), CassieError> {
    if let crate::sql::ast::QueryStatement::Select(select) = &statement.statement {
        validate_parameter_select(
            select,
            catalog,
            context,
            parameter_types,
            contextual_parameters,
            outer,
        )?;
    }
    Ok(())
}

fn validate_parameter_select(
    select: &SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
    contextual_parameters: bool,
    outer: &std::collections::HashMap<String, Schema>,
) -> Result<(), CassieError> {
    let plan = crate::planner::logical::LogicalPlan {
        command: None,
        source: select.source.clone(),
        collection: String::new(),
        ctes: select.ctes.clone(),
        distinct: select.distinct,
        distinct_on: select.distinct_on.clone(),
        projection: select.projection.clone(),
        filter: select.filter.clone(),
        group_by: select.group_by.clone(),
        having: select.having.clone(),
        order: select.order.clone(),
        limit: select.limit,
        offset: select.offset,
        set: select.set.clone(),
    };
    validate_plan_in_scope(
        &plan,
        catalog,
        context,
        parameter_types,
        contextual_parameters,
        outer,
    )
}

fn validate_nested_source(
    source: &crate::sql::ast::QuerySource,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
    contextual_parameters: bool,
    outer: &std::collections::HashMap<String, Schema>,
) -> Result<(), CassieError> {
    match source {
        crate::sql::ast::QuerySource::Subquery { select, .. } => validate_parameter_select(
            select,
            catalog,
            context,
            parameter_types,
            contextual_parameters,
            outer,
        ),
        crate::sql::ast::QuerySource::Join { left, right, .. } => {
            validate_nested_source(
                left,
                catalog,
                context,
                parameter_types,
                contextual_parameters,
                outer,
            )?;
            validate_nested_source(
                right,
                catalog,
                context,
                parameter_types,
                contextual_parameters,
                outer,
            )
        }
        _ => Ok(()),
    }
}

fn expression_contains_coalesce(expr: &Expr) -> bool {
    matches!(expr, Expr::Function(function) if function.name.eq_ignore_ascii_case("coalesce"))
        || expr.any_child(expression_contains_coalesce)
}

fn item_contains_coalesce(item: &SelectItem) -> bool {
    match item {
        SelectItem::Function { function, .. } => {
            function.name.eq_ignore_ascii_case("coalesce")
                || function.args.iter().any(expression_contains_coalesce)
        }
        SelectItem::Expr { expr, .. } => expression_contains_coalesce(expr),
        SelectItem::WindowFunction { function, .. } => function
            .args
            .iter()
            .chain(&function.partition_by)
            .chain(function.order_by.iter().map(|order| &order.expr))
            .any(expression_contains_coalesce),
        _ => false,
    }
}

fn select_contains_coalesce(select: &SelectStatement) -> bool {
    select.projection.iter().any(item_contains_coalesce)
        || select
            .filter
            .iter()
            .chain(&select.distinct_on)
            .chain(&select.group_by)
            .chain(&select.having)
            .chain(select.order.iter().map(|order| &order.expr))
            .any(expression_contains_coalesce)
        || source_contains_coalesce(&select.source)
        || select.ctes.iter().any(cte_contains_coalesce)
        || select
            .set
            .as_ref()
            .is_some_and(|set| select_contains_coalesce(&set.right))
}

fn statement_contains_coalesce(statement: &crate::sql::ast::ParsedStatement) -> bool {
    matches!(&statement.statement, crate::sql::ast::QueryStatement::Select(select) if select_contains_coalesce(select))
}

fn cte_contains_coalesce(cte: &crate::sql::ast::CommonTableExpression) -> bool {
    match &cte.query {
        crate::sql::ast::CteQuery::Simple(statement) => statement_contains_coalesce(statement),
        crate::sql::ast::CteQuery::Recursive {
            base, recursive, ..
        } => statement_contains_coalesce(base) || statement_contains_coalesce(recursive),
    }
}

fn source_contains_coalesce(source: &crate::sql::ast::QuerySource) -> bool {
    match source {
        crate::sql::ast::QuerySource::Subquery { select, .. } => select_contains_coalesce(select),
        crate::sql::ast::QuerySource::Join {
            left, right, on, ..
        } => {
            source_contains_coalesce(left)
                || source_contains_coalesce(right)
                || expression_contains_coalesce(on)
        }
        crate::sql::ast::QuerySource::TableFunction { function, .. } => {
            function.args.iter().any(expression_contains_coalesce)
        }
        _ => false,
    }
}

fn plan_contains_coalesce(plan: &crate::planner::logical::LogicalPlan) -> bool {
    if let Some(command) = &plan.command {
        let returning = match command {
            crate::planner::logical::LogicalCommand::Insert(statement) => &statement.returning,
            crate::planner::logical::LogicalCommand::Update(statement) => &statement.returning,
            crate::planner::logical::LogicalCommand::Delete(statement) => &statement.returning,
            _ => return false,
        };
        return returning.iter().any(item_contains_coalesce);
    }
    plan.projection.iter().any(item_contains_coalesce)
        || plan
            .filter
            .iter()
            .chain(&plan.distinct_on)
            .chain(&plan.group_by)
            .chain(&plan.having)
            .chain(plan.order.iter().map(|order| &order.expr))
            .any(expression_contains_coalesce)
        || source_contains_coalesce(&plan.source)
        || plan.ctes.iter().any(cte_contains_coalesce)
        || plan
            .set
            .as_ref()
            .is_some_and(|set| select_contains_coalesce(&set.right))
}

fn source_references_cte(source: &crate::sql::ast::QuerySource, name: &str) -> bool {
    match source {
        crate::sql::ast::QuerySource::Cte(candidate) => candidate.eq_ignore_ascii_case(name),
        crate::sql::ast::QuerySource::Subquery { select, .. } => {
            source_references_cte(&select.source, name)
                || select.ctes.iter().any(|cte| cte_references_name(cte, name))
        }
        crate::sql::ast::QuerySource::Join { left, right, .. } => {
            source_references_cte(left, name) || source_references_cte(right, name)
        }
        _ => false,
    }
}

fn cte_references_name(cte: &crate::sql::ast::CommonTableExpression, name: &str) -> bool {
    let references = |statement: &crate::sql::ast::ParsedStatement| {
        matches!(&statement.statement,
        crate::sql::ast::QueryStatement::Select(select) if source_references_cte(&select.source, name))
    };
    match &cte.query {
        crate::sql::ast::CteQuery::Simple(statement) => references(statement),
        crate::sql::ast::CteQuery::Recursive {
            base, recursive, ..
        } => references(base) || references(recursive),
    }
}
