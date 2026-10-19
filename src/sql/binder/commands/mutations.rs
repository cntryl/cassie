//! Binding for `TRUNCATE` and joined mutations: `UPDATE ... FROM`,
//! `DELETE ... USING` and `MERGE`.
//!
//! A joined mutation's source is bound once into a SELECT that projects each
//! source column under its qualified `relation.column` name; its expressions are
//! validated against the target joined to that source.

use super::super::{
    bind_select, coalesce_results::ResultTypes, resolve_relation_path, virtual_views, wildcard,
    BindingContext, CassieError, Catalog, CollectionSchema, CteScope, Expr, HashMap, HashSet,
    SelectItem,
};
use crate::sql::ast::{
    CommonTableExpression, IdentifierPath, MergeAction, MergeStatement, MutationSource,
    MutationSourceField, QuerySource, ReturningAliases, SelectStatement, TruncateStatement,
    MERGE_ACTION_COLUMN,
};
use crate::types::{DataType, FieldSchema};

pub(in crate::sql::binder) fn bind_truncate(
    mut statement: TruncateStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<TruncateStatement, CassieError> {
    for table in &mut statement.tables {
        let resolved = resolve_relation_path(table, catalog, context)?;
        if virtual_views::schema(&resolved).is_some() || catalog.get_view(&resolved).is_some() {
            return Err(crate::app::DiagnosticError::new(
                "42809",
                format!(
                    "\"{}\" is not a table",
                    crate::catalog::local_name(&resolved)
                ),
            )
            .into());
        }
        if !catalog.exists(&resolved) {
            return Err(CassieError::CollectionNotFound(resolved));
        }
        *table = IdentifierPath::parse(&resolved).map_err(CassieError::Planner)?;
    }
    Ok(statement)
}

/// Resolves a writable mutation target and its schema.
pub(super) fn resolve_target(
    table: &IdentifierPath,
    operation: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(String, CollectionSchema), CassieError> {
    let resolved = resolve_relation_path(table, catalog, context)?;
    if resolved.is_empty() {
        return Err(CassieError::Planner(format!(
            "{operation} requires a target table"
        )));
    }
    if virtual_views::schema(&resolved).is_some() || catalog.get_view(&resolved).is_some() {
        return Err(CassieError::Unsupported(format!(
            "relation '{resolved}' is read-only"
        )));
    }
    let schema = catalog
        .get_schema(&resolved)
        .ok_or_else(|| CassieError::CollectionNotFound(resolved.clone()))?;
    Ok((resolved, schema))
}

/// The qualifier target columns carry in a joined mutation row.
#[must_use]
pub(crate) fn target_qualifier(table: &str, alias: Option<&str>) -> String {
    alias.map_or_else(|| crate::catalog::local_name(table), str::to_string)
}

fn select_over(source: QuerySource, ctes: Vec<CommonTableExpression>) -> SelectStatement {
    SelectStatement {
        source,
        ctes,
        recursive: false,
        distinct: false,
        distinct_on: Vec::new(),
        projection: vec![SelectItem::Wildcard],
        filter: None,
        group_by: Vec::new(),
        having: None,
        order: Vec::new(),
        limit: None,
        offset: None,
        set: None,
    }
}

/// The bound joined scope of a mutation.
pub(super) struct JoinedScope {
    pub(super) types: ResultTypes,
}

/// Binds `source` into its qualified projection query and returns the scope
/// of the target joined to it.
pub(super) fn bind_mutation_source(
    source: &mut MutationSource,
    ctes: Vec<CommonTableExpression>,
    table: &str,
    qualifier: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<JoinedScope, CassieError> {
    let discovered = bind_select(
        select_over(source.source.clone(), ctes.clone()),
        catalog,
        &CteScope::default(),
        context,
    )?;
    let mut fields = Vec::new();
    source_leaves(&source.source, &discovered.ctes, catalog, &mut fields)?;
    if let Some(field) = fields
        .iter()
        .find(|field| field.qualifier.eq_ignore_ascii_case(qualifier))
    {
        return Err(crate::app::DiagnosticError::new(
            "42712",
            format!(
                "table name \"{}\" specified more than once",
                field.qualifier
            ),
        )
        .into());
    }
    let mut select = select_over(source.source.clone(), ctes);
    select.projection = fields
        .iter()
        .map(|field| SelectItem::Column {
            name: field.qualified_name(),
            alias: Some(field.qualified_name()),
        })
        .collect();
    let bound = bind_select(select, catalog, &CteScope::default(), context)?;
    let joined = QuerySource::Join {
        left: Box::new(QuerySource::Aliased {
            source: Box::new(QuerySource::Collection(
                IdentifierPath::parse(table).map_err(CassieError::Planner)?,
            )),
            alias: qualifier.to_string(),
            column_aliases: Vec::new(),
        }),
        right: Box::new(bound.source.clone()),
        kind: crate::sql::ast::JoinKind::Inner,
        on: Expr::BoolLiteral(true),
    };
    let types = ResultTypes::for_source(&joined, &bound.ctes, catalog, context)?;
    source.query = Some(Box::new(bound));
    source.fields = fields;
    Ok(JoinedScope { types })
}

fn source_leaves(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
    out: &mut Vec<MutationSourceField>,
) -> Result<(), CassieError> {
    match source {
        QuerySource::Join { left, right, .. } => {
            source_leaves(left, ctes, catalog, out)?;
            source_leaves(right, ctes, catalog, out)
        }
        QuerySource::Aliased {
            source: inner,
            alias,
            column_aliases,
        } => {
            let mut columns = leaf_columns(inner, ctes, catalog)?;
            for (column, alias) in columns.iter_mut().zip(column_aliases) {
                column.0.clone_from(alias);
            }
            push_leaf(out, alias, columns);
            Ok(())
        }
        QuerySource::Collection(name) => {
            let columns = leaf_columns(source, ctes, catalog)?;
            let qualifier = if names_cte(ctes, name) {
                name.to_string()
            } else {
                crate::catalog::local_name(name)
            };
            push_leaf(out, &qualifier, columns);
            Ok(())
        }
        QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => {
            let columns = leaf_columns(source, ctes, catalog)?;
            push_leaf(out, name, columns);
            Ok(())
        }
        QuerySource::Subquery { alias, .. } => {
            let columns = leaf_columns(source, ctes, catalog)?;
            push_leaf(out, alias, columns);
            Ok(())
        }
        QuerySource::Values { .. } => Err(CassieError::Parse(
            "VALUES in FROM must have an alias".to_string(),
        )),
        QuerySource::SingleRow => Ok(()),
    }
}

fn push_leaf(
    out: &mut Vec<MutationSourceField>,
    qualifier: &str,
    columns: Vec<(String, DataType)>,
) {
    out.extend(
        columns
            .into_iter()
            .map(|(column, data_type)| MutationSourceField {
                qualifier: qualifier.to_ascii_lowercase(),
                column,
                data_type,
            }),
    );
}

fn names_cte(ctes: &[CommonTableExpression], name: &IdentifierPath) -> bool {
    let name = name.to_string();
    ctes.iter().any(|cte| cte.name.eq_ignore_ascii_case(&name))
}

fn leaf_columns(
    source: &QuerySource,
    ctes: &[CommonTableExpression],
    catalog: &Catalog,
) -> Result<Vec<(String, DataType)>, CassieError> {
    if let QuerySource::Collection(name) = source {
        if names_cte(ctes, name) {
            return leaf_columns(&QuerySource::Cte(name.to_string()), ctes, catalog);
        }
        if let Some(schema) = catalog.get_schema(name) {
            return Ok(schema
                .fields
                .into_iter()
                .map(|field| {
                    (
                        crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key(),
                        field.data_type,
                    )
                })
                .collect());
        }
    }
    let fields = wildcard::wildcard_output_fields(source, ctes, catalog, &HashMap::new())?;
    Ok(fields
        .into_iter()
        .map(|field| {
            let column = field
                .name
                .rsplit_once('.')
                .map_or(field.name.as_str(), |(_, column)| column)
                .to_string();
            (column, field.data_type)
        })
        .collect())
}

/// Fields a mutation row carries beyond its relations.
pub(super) fn row_image_fields(
    schema: &CollectionSchema,
    aliases: Option<&ReturningAliases>,
    merge: bool,
) -> Vec<FieldSchema> {
    let mut fields = Vec::new();
    if merge {
        fields.push(FieldSchema {
            name: MERGE_ACTION_COLUMN.to_string(),
            data_type: DataType::Text,
            nullable: false,
        });
    }
    let default_aliases = ReturningAliases::default();
    let aliases = aliases.unwrap_or(&default_aliases);
    for qualifier in [&aliases.old, &aliases.new] {
        for field in &schema.fields {
            fields.push(FieldSchema {
                name: format!(
                    "{qualifier}.{}",
                    crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key()
                ),
                data_type: field.data_type.clone(),
                nullable: true,
            });
        }
    }
    fields
}

/// Replaces `merge_action()` with the row entry that carries it and names the
/// output column like PostgreSQL does.
pub(super) fn rewrite_merge_action(returning: &mut [SelectItem]) {
    for item in returning {
        match item {
            SelectItem::Function { function, alias } if is_merge_action(function) => {
                *item = SelectItem::Column {
                    name: MERGE_ACTION_COLUMN.to_string(),
                    alias: Some(alias.clone().unwrap_or_else(|| "merge_action".to_string())),
                };
            }
            SelectItem::Expr { expr, alias } => {
                if let Expr::Function(function) = expr {
                    if is_merge_action(function) && alias.is_none() {
                        *alias = Some("merge_action".to_string());
                    }
                }
                *expr = replace_merge_action(expr);
            }
            _ => {}
        }
    }
}

fn is_merge_action(function: &crate::sql::ast::FunctionCall) -> bool {
    function.args.is_empty()
        && matches!(
            function.name.to_ascii_lowercase().as_str(),
            "merge_action" | "pg_catalog.merge_action"
        )
}

fn replace_merge_action(expr: &Expr) -> Expr {
    match expr {
        Expr::Function(function) if is_merge_action(function) => {
            Expr::Column(MERGE_ACTION_COLUMN.to_string())
        }
        _ => expr.map_children(replace_merge_action),
    }
}

/// Validates `SET` targets and values against the target and joined scope.
pub(super) fn bind_assignments(
    assignments: &mut [(String, Expr)],
    row_subquery_columns: &mut [Vec<String>],
    schema: &CollectionSchema,
    table: &str,
    scope: &JoinedScope,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let mut seen = HashSet::new();
    let mut resolve = |column: &mut String| -> Result<DataType, CassieError> {
        let requested = column.trim().to_string();
        let declared = schema
            .fields
            .iter()
            .find(|field| {
                crate::sql::ColumnIdentifierPath::parse(&requested)
                    .is_ok_and(|reference| reference.matches_field_name(&field.name))
            })
            .ok_or_else(|| {
                CassieError::UndefinedColumn(format!(
                    "column \"{requested}\" of relation \"{}\" does not exist",
                    crate::catalog::local_name(table)
                ))
            })?;
        if !seen.insert(declared.name.clone()) {
            return Err(CassieError::Parse(format!(
                "multiple assignments to same column \"{requested}\""
            )));
        }
        column.clone_from(&declared.name);
        Ok(declared.data_type.clone())
    };
    for (column, expression) in assignments {
        let data_type = resolve(column)?;
        if expression.is_default_assignment() {
            continue;
        }
        super::super::boolean_contexts::validate_value(
            expression,
            &scope.types,
            Some(&data_type),
            catalog,
            context,
        )?;
    }
    for columns in row_subquery_columns {
        for column in columns {
            resolve(column)?;
        }
    }
    Ok(())
}

pub(super) fn bind_predicate(
    predicate: Option<&mut Expr>,
    scope: &JoinedScope,
    label: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let Some(predicate) = predicate else {
        return Ok(());
    };
    super::super::boolean_contexts::validate_predicate(
        predicate,
        &scope.types,
        label,
        catalog,
        context,
    )
}

pub(super) fn bind_returning(
    returning: &mut Vec<SelectItem>,
    target: (&CollectionSchema, &str),
    source: Option<&MutationSource>,
    scope: &JoinedScope,
) -> Result<(), CassieError> {
    expand_returning_wildcard(returning, target, source);
    for item in returning.iter() {
        scope.types.item(item)?;
    }
    Ok(())
}

/// `RETURNING *` names every target column, then every source column.
fn expand_returning_wildcard(
    returning: &mut Vec<SelectItem>,
    (schema, qualifier): (&CollectionSchema, &str),
    source: Option<&MutationSource>,
) {
    if !returning
        .iter()
        .any(|item| matches!(item, SelectItem::Wildcard))
    {
        return;
    }
    let mut expanded = Vec::with_capacity(returning.len() + schema.fields.len());
    for item in returning.drain(..) {
        if !matches!(item, SelectItem::Wildcard) {
            expanded.push(item);
            continue;
        }
        for field in &schema.fields {
            let column =
                crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key();
            expanded.push(SelectItem::Column {
                name: format!("{qualifier}.{column}"),
                alias: Some(field.name.clone()),
            });
        }
        for field in source.map_or(&[][..], |source| source.fields.as_slice()) {
            expanded.push(SelectItem::Column {
                name: field.qualified_name(),
                alias: Some(field.column.clone()),
            });
        }
    }
    *returning = expanded;
}

pub(in crate::sql::binder) fn bind_merge(
    mut statement: MergeStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<MergeStatement, CassieError> {
    let (table, schema) = resolve_target(&statement.table, "MERGE", catalog, context)?;
    let qualifier = target_qualifier(&table, statement.alias.as_deref());
    infer_row_image_aliases(
        &statement.returning,
        &mut statement.returning_aliases,
        &qualifier,
    );
    let ctes = std::mem::take(&mut statement.ctes);
    let scope = bind_mutation_source(
        &mut statement.source,
        ctes,
        &table,
        &qualifier,
        catalog,
        context,
    )?;
    let scope = JoinedScope {
        types: scope.types.with_extra_fields(row_image_fields(
            &schema,
            statement.returning_aliases.as_ref(),
            true,
        )),
    };
    super::super::boolean_contexts::validate_predicate(
        &mut statement.on,
        &scope.types,
        "MERGE ON",
        catalog,
        context,
    )?;
    for clause in &mut statement.clauses {
        bind_predicate(
            clause.condition.as_mut(),
            &scope,
            "MERGE WHEN",
            catalog,
            context,
        )?;
        match &mut clause.action {
            MergeAction::Update {
                assignments,
                row_subqueries,
            } => {
                let mut columns = row_subqueries
                    .iter()
                    .map(|assignment| assignment.columns.clone())
                    .collect::<Vec<_>>();
                bind_assignments(
                    assignments,
                    &mut columns,
                    &schema,
                    &table,
                    &scope,
                    catalog,
                    context,
                )?;
                for (assignment, columns) in row_subqueries.iter_mut().zip(columns) {
                    assignment.columns = columns;
                }
            }
            MergeAction::Insert {
                columns, values, ..
            } => bind_merge_insert(
                columns,
                values.as_mut(),
                &schema,
                &table,
                &scope,
                catalog,
                context,
            )?,
            MergeAction::Delete | MergeAction::DoNothing => {}
        }
    }
    rewrite_merge_action(&mut statement.returning);
    bind_returning(
        &mut statement.returning,
        (&schema, &qualifier),
        Some(&statement.source),
        &scope,
    )?;
    statement.table = IdentifierPath::parse(&table).map_err(CassieError::Planner)?;
    statement.alias = Some(qualifier);
    Ok(statement)
}

fn bind_merge_insert(
    columns: &mut [String],
    values: Option<&mut Vec<Expr>>,
    schema: &CollectionSchema,
    table: &str,
    scope: &JoinedScope,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let mut seen = HashSet::new();
    let mut types = Vec::with_capacity(columns.len());
    for column in columns.iter_mut() {
        let declared = schema
            .fields
            .iter()
            .find(|field| {
                crate::sql::ColumnIdentifierPath::parse(column)
                    .is_ok_and(|reference| reference.matches_field_name(&field.name))
            })
            .ok_or_else(|| {
                CassieError::UndefinedColumn(format!(
                    "column \"{column}\" of relation \"{}\" does not exist",
                    crate::catalog::local_name(table)
                ))
            })?;
        if !seen.insert(declared.name.clone()) {
            return Err(crate::app::DiagnosticError::new(
                "42701",
                format!("column \"{column}\" specified more than once"),
            )
            .into());
        }
        column.clone_from(&declared.name);
        types.push(declared.data_type.clone());
    }
    let Some(values) = values else {
        return Ok(());
    };
    if columns.is_empty() && values.len() > schema.fields.len() {
        return Err(CassieError::Parse(
            "INSERT has more expressions than target columns".to_string(),
        ));
    }
    for (index, value) in values.iter_mut().enumerate() {
        if value.is_default_assignment() {
            continue;
        }
        let expected = types.get(index).cloned().or_else(|| {
            schema
                .fields
                .get(index)
                .map(|field| field.data_type.clone())
        });
        super::super::boolean_contexts::validate_value(
            value,
            &scope.types,
            expected.as_ref(),
            catalog,
            context,
        )?;
    }
    Ok(())
}

/// PostgreSQL 18 names the row images `old` and `new` without a `WITH` clause
/// unless the target itself carries one of those names.
pub(in crate::sql::binder) fn infer_row_image_aliases(
    returning: &[SelectItem],
    aliases: &mut Option<ReturningAliases>,
    target_qualifier: &str,
) {
    if aliases.is_some()
        || ["old", "new"]
            .iter()
            .any(|name| target_qualifier.eq_ignore_ascii_case(name))
    {
        return;
    }
    let defaults = ReturningAliases::default();
    let references = |name: &str| {
        crate::sql::ColumnIdentifierPath::parse(name).is_ok_and(|column| {
            let field = column.field_lookup_key();
            column
                .lookup_key()
                .strip_suffix(&format!(".{field}"))
                .is_some_and(|qualifier| qualifier == defaults.old || qualifier == defaults.new)
        })
    };
    let expression_references = |expr: &Expr| {
        expr.any_descendant_or_self(
            &mut |node| matches!(node, Expr::Column(name) if references(name)),
        )
    };
    let found = returning.iter().any(|item| match item {
        SelectItem::Column { name, .. } => references(name),
        SelectItem::Expr { expr, .. } => expression_references(expr),
        SelectItem::Function { function, .. } => function.args.iter().any(expression_references),
        SelectItem::Wildcard | SelectItem::WindowFunction { .. } => false,
    });
    if found {
        *aliases = Some(defaults);
    }
}
