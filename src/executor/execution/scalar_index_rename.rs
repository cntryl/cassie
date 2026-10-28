//! Scalar index metadata rename without rewriting stored index entries.
use super::{empty_command, Cassie, QueryError, QueryExecutionControls, QueryResult};
use crate::app::{CassieError, CatalogObjectKind};
use crate::catalog::{local_name, FieldConstraint, IndexKind, IndexMeta};
use crate::sql::ast::AlterIndexStatement;

pub(super) fn rename(
    cassie: &Cassie,
    statement: &AlterIndexStatement,
    new_name: &str,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let output = empty_command("ALTER INDEX", controls)?;
    if statement.collection.is_empty() && statement.if_exists {
        return Ok(output);
    }
    let logical = cassie
        .catalog
        .materialized_projection_for_output(&statement.collection)
        .map(|projection| projection.collection);
    let execute = || rename_gated(cassie, statement, new_name, controls);
    if let Some(logical) = logical {
        cassie.midge.with_collection_gates(&[logical], execute)?;
    } else {
        execute()?;
    }
    Ok(output)
}

fn rename_gated(
    cassie: &Cassie,
    statement: &AlterIndexStatement,
    new_name: &str,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    let collection = cassie
        .catalog
        .materialized_projection_for_output(&statement.collection)
        .and_then(|projection| projection.active_output_collection().map(str::to_owned))
        .unwrap_or_else(|| statement.collection.clone());
    let canonical = cassie.midge.canonical_collection_name(&collection);
    cassie.midge.with_collection_gates(&[canonical], || {
        controls.check_interruption()?;
        let Some(index) = cassie.midge.get_index(&collection, &statement.name)? else {
            if statement.if_exists {
                return Ok(());
            }
            return Err(CassieError::CatalogObjectNotFound {
                kind: CatalogObjectKind::Index,
                name: statement.name.clone(),
            });
        };
        if index.kind != IndexKind::Scalar {
            return Err(CassieError::Unsupported(
                "ALTER INDEX RENAME supports scalar indexes only".into(),
            ));
        }
        if cassie.catalog.relation_exists(new_name)
            || cassie.catalog.all_indexes_snapshot().iter().any(|other| {
                let Some(owner) = active_owner(cassie, other) else {
                    return false;
                };
                let scoped = scoped_index_name(&owner, &other.name);
                cassie.midge.canonical_collection_name(&scoped)
                    == cassie.midge.canonical_collection_name(new_name)
            })
        {
            return Err(CassieError::CatalogObjectAlreadyExists {
                kind: CatalogObjectKind::Index,
                name: new_name.to_string(),
            });
        }
        let mut constraints = cassie.catalog.get_constraints(&collection);
        rename_constraint_ownership(&mut constraints, &index, &collection, new_name)?;
        controls.check_interruption()?;
        let mut updated = cassie
            .midge
            .rename_index_and_constraints_with_held_collection_gate(
                &collection,
                &index.name,
                new_name,
                &constraints,
            )?;
        cassie.catalog.unregister_index(&collection, &index.name);
        updated.collection.clone_from(&collection);
        cassie.catalog.register_index(updated);
        cassie
            .catalog
            .register_constraints(&collection, constraints);
        Ok(())
    })
}

fn active_owner(cassie: &Cassie, index: &IndexMeta) -> Option<String> {
    let Some(projection) = cassie
        .catalog
        .materialized_projection_for_output(&index.collection)
    else {
        return Some(index.collection.clone());
    };
    if cassie
        .catalog
        .is_sql_materialized_view(&projection.collection)
        && projection.active_output_collection() != Some(index.collection.as_str())
    {
        return None;
    }
    Some(projection.collection)
}

fn rename_constraint_ownership(
    constraints: &mut [FieldConstraint],
    index: &IndexMeta,
    collection: &str,
    new_name: &str,
) -> Result<(), CassieError> {
    let old = local_name(&index.name);
    let new = local_name(new_name);
    let owns_constraint = |constraint: &FieldConstraint| {
        (constraint.primary_key || constraint.unique)
            && (constraint.unique_constraint_name(collection) == old
                || constraint
                    .constraint_index_name
                    .as_ref()
                    .is_some_and(|name| name == &index.name))
    };
    if constraints.iter().any(owns_constraint)
        && constraints.iter().any(|constraint| {
            !owns_constraint(constraint)
                && [
                    constraint.primary_key_name.as_ref(),
                    constraint.unique_name.as_ref(),
                    constraint.check_name.as_ref(),
                    constraint.foreign_key_name.as_ref(),
                ]
                .into_iter()
                .flatten()
                .any(|name| name == &new)
        })
    {
        return Err(crate::app::DiagnosticError::new(
            "42710",
            format!("constraint '{new}' already exists"),
        )
        .into());
    }
    for constraint in constraints {
        if owns_constraint(constraint) {
            if constraint.primary_key {
                constraint.primary_key_name = Some(new.clone());
            } else {
                constraint.unique_name = Some(new.clone());
            }
            constraint.constraint_index_name = Some(new_name.to_string());
        }
    }
    Ok(())
}

fn scoped_index_name(owner: &str, name: &str) -> String {
    let path =
        crate::sql::ast::IdentifierPath::parse(owner).expect("catalog owner is a valid name");
    let mut parts = path.values().map(str::to_owned).collect::<Vec<_>>();
    *parts.last_mut().expect("owner has a local component") = local_name(name);
    crate::sql::ast::IdentifierPath::from_values(parts).into_string()
}
