//! `INSERT ... ON CONFLICT` target inference and `DO UPDATE` validation.

use super::super::{
    validate_expression, validate_function_calls, BindingContext, CassieError, Catalog,
    CollectionSchema, Expr, HashSet,
};
use crate::sql::ast::IdentifierPath;

pub(super) fn bind_on_conflict(
    on_conflict: Option<&mut crate::sql::ast::InsertConflictClause>,
    schema: &CollectionSchema,
    table: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    if let Some(on_conflict) = on_conflict {
        let mut normalized_target = Vec::with_capacity(on_conflict.target_fields.len());
        for field in &on_conflict.target_fields {
            let field_name = field.trim();
            if field_name.is_empty() {
                return Err(CassieError::Planner(
                    "ON CONFLICT target fields cannot be empty".into(),
                ));
            }
            let Some(declared) = schema.fields.iter().find(|candidate| {
                crate::sql::ColumnIdentifierPath::parse(field_name)
                    .is_ok_and(|reference| reference.matches_field_name(&candidate.name))
            }) else {
                return Err(CassieError::Planner(format!(
                    "ON CONFLICT target column '{field_name}' does not exist in '{table}'"
                )));
            };
            normalized_target.push(declared.name.clone());
        }
        on_conflict.target_fields = normalized_target;
        infer_arbiter(on_conflict, table, catalog)?;

        if matches!(
            on_conflict.action,
            crate::sql::ast::InsertConflictAction::DoUpdate { .. }
        ) && on_conflict.target_fields.is_empty()
            && on_conflict.constraint.is_none()
        {
            return Err(CassieError::Planner(
                "ON CONFLICT DO UPDATE requires an explicit conflict target".into(),
            ));
        }

        if !on_conflict.target_fields.is_empty()
            && !conflict_target_supported(catalog, table, &on_conflict.target_fields)
        {
            return Err(CassieError::Planner(format!(
                "ON CONFLICT target {:?} does not match a unique or primary key on '{table}'",
                on_conflict.target_fields
            )));
        }

        if let crate::sql::ast::InsertConflictAction::DoUpdate {
            assignments,
            filter,
            ..
        } = &mut on_conflict.action
        {
            validate_conflict_update(
                assignments,
                filter.as_mut(),
                schema,
                table,
                catalog,
                context,
            )?;
            let mut seen = assignments
                .iter()
                .map(|(column, _)| column.clone())
                .collect::<HashSet<_>>();
            for assignment in &mut on_conflict.update_row_subqueries {
                for column in &mut assignment.columns {
                    let declared = schema
                        .fields
                        .iter()
                        .find(|field| {
                            crate::sql::ColumnIdentifierPath::parse(column)
                                .is_ok_and(|reference| reference.matches_field_name(&field.name))
                        })
                        .ok_or_else(|| {
                            CassieError::Planner(format!(
                                "ON CONFLICT assignment target '{column}' does not exist in '{table}'"
                            ))
                        })?;
                    if !seen.insert(declared.name.clone()) {
                        return Err(CassieError::Parse(format!(
                            "multiple assignments to same column \"{column}\""
                        )));
                    }
                    column.clone_from(&declared.name);
                }
            }
        }
    }
    Ok(())
}

fn validate_conflict_update(
    assignments: &mut [(String, Expr)],
    filter: Option<&mut Expr>,
    schema: &CollectionSchema,
    table: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let mut known_fields = schema
        .fields
        .iter()
        .flat_map(|field| {
            let name = crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key();
            [name.clone(), format!("excluded.{name}")]
        })
        .collect::<HashSet<_>>();
    let local_table = crate::catalog::local_name(table);
    known_fields.extend(super::super::select::base_table_fields(table, schema));
    known_fields.extend(super::super::select::base_table_fields(
        &local_table,
        schema,
    ));
    for field in &schema.fields {
        let field = crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key();
        known_fields.insert(format!("{table}.{field}"));
        known_fields.insert(format!("{local_table}.{field}"));
    }

    let result_types = super::super::coalesce_results::ResultTypes::for_source(
        &crate::sql::ast::QuerySource::Collection(
            IdentifierPath::parse(table).map_err(CassieError::Planner)?,
        ),
        &[],
        catalog,
        context,
    )?
    .with_excluded_fields(schema);
    let mut seen = HashSet::new();
    let mut functions = Vec::new();
    for (target, expression) in assignments {
        let requested = target.trim();
        let Some(declared) = schema.fields.iter().find(|field| {
            crate::sql::ColumnIdentifierPath::parse(requested)
                .is_ok_and(|reference| reference.matches_field_name(&field.name))
        }) else {
            return Err(CassieError::Planner(format!(
                "ON CONFLICT assignment target '{requested}' does not exist in '{table}'"
            )));
        };
        let normalized = declared.name.clone();
        if !seen.insert(normalized.clone()) {
            return Err(CassieError::Planner(format!(
                "ON CONFLICT assignment target '{normalized}' is duplicated"
            )));
        }
        if expression.is_default_assignment() {
            *target = normalized;
            continue;
        }
        validate_expression(expression, &known_fields, &HashSet::new(), false)?;
        super::super::boolean_contexts::validate_value(
            expression,
            &result_types,
            Some(&declared.data_type),
            catalog,
            context,
        )?;
        super::super::collect_expr(expression, &mut functions);
        *target = normalized;
    }
    if let Some(filter) = filter.as_deref() {
        validate_expression(filter, &known_fields, &HashSet::new(), false)?;
        super::super::collect_expr(filter, &mut functions);
    }
    validate_function_calls(functions, catalog, context)?;
    if let Some(filter) = filter {
        super::super::boolean_contexts::validate_predicate(
            filter,
            &result_types,
            "ON CONFLICT WHERE",
            catalog,
            context,
        )?;
    }
    Ok(())
}

fn conflict_target_supported(catalog: &Catalog, table: &str, target_fields: &[String]) -> bool {
    if target_fields.is_empty() {
        return true;
    }

    // PostgreSQL infers the arbiter from the set of target columns, so
    // `ON CONFLICT (b, a)` matches a key declared on `(a, b)`.
    let column_set = |fields: &mut dyn Iterator<Item = &String>| {
        let mut set = fields
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();
        set.sort();
        set.dedup();
        set
    };
    let normalized_target = column_set(&mut target_fields.iter());

    let constraints = catalog.get_constraints(table);
    if constraints.iter().any(|constraint| {
        crate::catalog::enforces_single_column_uniqueness(constraint, &constraints)
            && normalized_target == [constraint.field.clone()]
    }) {
        return true;
    }

    catalog
        .list_indexes(table)
        .into_iter()
        .filter(|index| index.unique && index.kind == crate::catalog::IndexKind::Scalar)
        .any(|index| column_set(&mut index.normalized_fields().iter()) == normalized_target)
}

/// Resolves `ON CONSTRAINT name` and expression or partial-index inference to
/// the arbiter: its key columns, or the name of the unique index whose
/// expressions or predicate the executor must evaluate.
fn infer_arbiter(
    on_conflict: &mut crate::sql::ast::InsertConflictClause,
    table: &str,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    if let Some(name) = on_conflict.constraint.take() {
        match named_arbiter(catalog, table, &name)? {
            Arbiter::Fields(fields) => on_conflict.target_fields = fields,
            Arbiter::Index(index) => on_conflict.constraint = Some(index),
        }
        return Ok(());
    }
    if on_conflict.target_expressions.is_empty() && on_conflict.target_predicate.is_none() {
        return Ok(());
    }
    let index = catalog
        .list_indexes(table)
        .into_iter()
        .filter(|index| index.unique && index.kind == crate::catalog::IndexKind::Scalar)
        .find(|index| index_matches_inference(index, on_conflict))
        .ok_or_else(|| {
            CassieError::from(crate::app::DiagnosticError::new(
                "42P10",
                "there is no unique or exclusion constraint matching the ON CONFLICT specification",
            ))
        })?;
    if index.expressions.is_empty() && index.predicate.is_none() {
        on_conflict.target_fields = index.normalized_fields();
    } else {
        on_conflict.constraint = Some(index.name);
    }
    Ok(())
}

enum Arbiter {
    Fields(Vec<String>),
    Index(String),
}

fn named_arbiter(catalog: &Catalog, table: &str, name: &str) -> Result<Arbiter, CassieError> {
    let local = crate::catalog::local_name(table);
    let constraints = catalog.get_constraints(table);
    for constraint in &constraints {
        if !crate::catalog::enforces_single_column_uniqueness(constraint, &constraints) {
            continue;
        }
        let postgres_name = if constraint.primary_key {
            format!("{local}_pkey")
        } else {
            format!("{local}_{}_key", constraint.field)
        };
        if constraint
            .unique_constraint_name(table)
            .eq_ignore_ascii_case(name)
            || postgres_name.eq_ignore_ascii_case(name)
        {
            return Ok(Arbiter::Fields(vec![constraint.field.clone()]));
        }
    }
    let indexes = crate::catalog::composite_unique_indexes(table, &constraints)
        .into_iter()
        .chain(catalog.list_indexes(table))
        .filter(|index| index.unique && index.kind == crate::catalog::IndexKind::Scalar);
    for index in indexes {
        if !index.name.eq_ignore_ascii_case(name) {
            continue;
        }
        return Ok(
            if index.expressions.is_empty() && index.predicate.is_none() {
                Arbiter::Fields(index.normalized_fields())
            } else {
                Arbiter::Index(index.name)
            },
        );
    }
    Err(crate::app::DiagnosticError::new(
        "42704",
        format!("constraint \"{name}\" for table \"{local}\" does not exist"),
    )
    .into())
}

fn index_matches_inference(
    index: &crate::catalog::IndexMeta,
    on_conflict: &crate::sql::ast::InsertConflictClause,
) -> bool {
    let mut fields = index.normalized_fields();
    if !index.expressions.is_empty() && index.fields.is_empty() {
        fields.clear();
    }
    let mut requested = on_conflict.target_fields.clone();
    fields.sort();
    requested.sort();
    if fields != requested {
        return false;
    }
    let Some(expressions) = index
        .expressions
        .iter()
        .map(|raw| stored_expression(raw))
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    if expressions.len() != on_conflict.target_expressions.len()
        || !on_conflict.target_expressions.iter().all(|target| {
            expressions
                .iter()
                .any(|expression| expression.structurally_eq(target))
        })
    {
        return false;
    }
    match (&index.predicate, &on_conflict.target_predicate) {
        (None, _) => true,
        (Some(raw), Some(requested)) => {
            stored_expression(raw).is_some_and(|predicate| predicate.structurally_eq(requested))
        }
        (Some(_), None) => false,
    }
}

/// An index expression as the catalog stores it: serialized, or SQL text.
fn stored_expression(raw: &str) -> Option<Expr> {
    serde_json::from_str(raw)
        .ok()
        .or_else(|| crate::sql::parser::parse_expression(raw).ok())
}
