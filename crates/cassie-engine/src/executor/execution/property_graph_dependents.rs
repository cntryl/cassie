//! Lifecycle of tables mapped by property graphs (#1255).
//!
//! A graph stores no derived state, so the only thing a table change can break
//! is the definition itself: a mapped table, a referenced column, or the unique
//! key that makes a mapping's `KEY` an identity. Those changes are rejected
//! with `2BP01`. Renames are followed, keeping every identity.

use super::property_graph_command::Source;
use super::schema_command::schema_namespace_command::{
    object_in_schema, rewrite_relation_name_from_map, RelationRenames,
};
use super::{Cassie, QueryError};
use crate::app::DiagnosticError;
use crate::catalog::{FieldConstraint, GraphKeyColumn, PropertyGraphMeta};

const DEPENDENT_OBJECTS: &str = "2BP01";
const UNSUPPORTED: &str = "0A000";

/// Display names of the graphs that depend on something, for the error detail.
fn dependent_detail(graphs: &[PropertyGraphMeta]) -> String {
    let names = graphs
        .iter()
        .map(|graph| format!("property graph {}", crate::catalog::local_name(&graph.name)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("dependent objects: {names}")
}

fn dependency_error(subject: &str, graphs: &[PropertyGraphMeta], cascade: bool) -> QueryError {
    let diagnostic = if cascade {
        DiagnosticError::new(
            UNSUPPORTED,
            format!("CASCADE cannot remove the property graphs that depend on {subject}"),
        )
    } else {
        DiagnosticError::new(
            DEPENDENT_OBJECTS,
            format!("cannot change {subject} because property graphs depend on it"),
        )
    };
    QueryError::Cassie(diagnostic.with_detail(dependent_detail(graphs)).into())
}

/// Whether a stored mapping table name and `canonical` name the same table. An instance
/// that does not enforce database scoping stores names unqualified, so both are compared
/// in canonical form.
fn same_table(cassie: &Cassie, stored: &str, canonical: &str) -> bool {
    cassie
        .midge
        .canonical_collection_name(stored)
        .eq_ignore_ascii_case(canonical)
}

/// The graphs that map `table`, with the table's name as those graphs store it.
fn graphs_on(cassie: &Cassie, table: &str) -> (String, Vec<PropertyGraphMeta>) {
    let canonical = cassie.midge.canonical_collection_name(table);
    let mut stored = None;
    let mut graphs = cassie.catalog.list_property_graphs();
    graphs.retain(|graph| {
        let mapped = graph
            .mapped_tables()
            .into_iter()
            .find(|mapped| same_table(cassie, mapped, &canonical))
            .map(str::to_owned);
        let maps = mapped.is_some();
        stored = stored.take().or(mapped);
        maps
    });
    (stored.unwrap_or(canonical), graphs)
}

/// Whether a mapping of `table` reads `column` as a key, endpoint or property.
fn references_column(graph: &PropertyGraphMeta, table: &str, column: &str) -> bool {
    let same = |candidate: &str| candidate.eq_ignore_ascii_case(column);
    let on_table = |candidate: &str| candidate.eq_ignore_ascii_case(table);
    let labelled = |labels: &[crate::catalog::GraphLabelMeta]| {
        labels
            .iter()
            .flat_map(|label| &label.properties)
            .any(|property| same(&property.column))
    };
    graph.references_key_column(table, column)
        || graph
            .vertices
            .iter()
            .any(|vertex| on_table(&vertex.table) && labelled(&vertex.labels))
        || graph
            .edges
            .iter()
            .any(|edge| on_table(&edge.table) && labelled(&edge.labels))
}

/// The key tuples that mappings of `table` declare.
fn mapped_keys<'graph>(
    graph: &'graph PropertyGraphMeta,
    table: &str,
) -> Vec<&'graph [GraphKeyColumn]> {
    graph
        .vertices
        .iter()
        .filter(|vertex| vertex.table.eq_ignore_ascii_case(table))
        .map(|vertex| vertex.key.as_slice())
        .chain(
            graph
                .edges
                .iter()
                .filter(|edge| edge.table.eq_ignore_ascii_case(table))
                .map(|edge| edge.key.as_slice()),
        )
        .collect()
}

pub(super) fn reject_table_drop(
    cassie: &Cassie,
    table: &str,
    cascade: bool,
) -> Result<(), QueryError> {
    let (canonical, graphs) = graphs_on(cassie, table);
    if graphs.is_empty() {
        return Ok(());
    }
    let subject = format!("table {}", crate::catalog::local_name(&canonical));
    Err(dependency_error(&subject, &graphs, cascade))
}

/// Rejects dropping or retyping a column that a mapping reads.
pub(super) fn reject_column_change(
    cassie: &Cassie,
    table: &str,
    column: &str,
    cascade: bool,
) -> Result<(), QueryError> {
    let (canonical, mut graphs) = graphs_on(cassie, table);
    graphs.retain(|graph| references_column(graph, &canonical, column));
    if graphs.is_empty() {
        return Ok(());
    }
    let subject = format!(
        "column {column} of table {}",
        crate::catalog::local_name(&canonical)
    );
    Err(dependency_error(&subject, &graphs, cascade))
}

/// Rejects making a mapped key column nullable.
pub(super) fn reject_key_nullable(
    cassie: &Cassie,
    table: &str,
    column: &str,
) -> Result<(), QueryError> {
    let (canonical, mut graphs) = graphs_on(cassie, table);
    graphs.retain(|graph| {
        mapped_keys(graph, &canonical).iter().any(|key| {
            key.iter()
                .any(|part| part.name.eq_ignore_ascii_case(column))
        })
    });
    if graphs.is_empty() {
        return Ok(());
    }
    let subject = format!(
        "the NOT NULL constraint of key column {column} of table {}",
        crate::catalog::local_name(&canonical)
    );
    Err(dependency_error(&subject, &graphs, false))
}

/// Rejects a constraint or index removal that would leave a mapped key nullable
/// or without a non-partial unique key on exactly its columns. `constraints` is the
/// table's constraint set after the removal; `dropped_indexes` no longer count.
pub(super) fn reject_unbacked_keys(
    cassie: &Cassie,
    table: &str,
    constraints: &[FieldConstraint],
    dropped_indexes: &[&str],
) -> Result<(), QueryError> {
    let (canonical, mut graphs) = graphs_on(cassie, table);
    if graphs.is_empty() {
        return Ok(());
    }
    let Some(mut source) = Source::of_table(cassie, &canonical) else {
        return Ok(());
    };
    source.replace_constraints(constraints.to_vec());
    source.forget_indexes(dropped_indexes);
    graphs.retain(|graph| {
        mapped_keys(graph, &canonical).iter().any(|key| {
            !source.has_unique_key(key) || key.iter().any(|part| !source.is_not_null(&part.name))
        })
    });
    if graphs.is_empty() {
        return Ok(());
    }
    let subject = format!(
        "the key of table {}",
        crate::catalog::local_name(&canonical)
    );
    Err(dependency_error(&subject, &graphs, false))
}

/// Stores every graph that `update` changed, under its possibly new name.
fn update_graphs(
    cassie: &Cassie,
    mut update: impl FnMut(&mut PropertyGraphMeta) -> bool,
) -> Result<(), QueryError> {
    for previous in cassie.catalog.list_property_graphs() {
        let mut graph = previous.clone();
        if !update(&mut graph) {
            continue;
        }
        cassie
            .midge
            .rename_property_graph(&previous.name, &graph)
            .map_err(QueryError::from)?;
        cassie.catalog.unregister_property_graph(&previous.name);
        cassie.catalog.register_property_graph(graph);
    }
    Ok(())
}

fn rewrite(value: &mut String, next: impl FnOnce(&str) -> Option<String>) -> bool {
    match next(value) {
        Some(next) if next != *value => {
            *value = next;
            true
        }
        _ => false,
    }
}

fn rewrite_tables(
    graph: &mut PropertyGraphMeta,
    mut next: impl FnMut(&str) -> Option<String>,
) -> bool {
    let mut changed = false;
    for vertex in &mut graph.vertices {
        changed |= rewrite(&mut vertex.table, &mut next);
    }
    for edge in &mut graph.edges {
        changed |= rewrite(&mut edge.table, &mut next);
    }
    changed
}

/// Repoints mappings at a renamed or moved table.
pub(super) fn rename_table(
    cassie: &Cassie,
    table: &str,
    next_table: &str,
) -> Result<(), QueryError> {
    let current = cassie.midge.canonical_collection_name(table);
    let next = cassie.midge.canonical_collection_name(next_table);
    update_graphs(cassie, |graph| {
        rewrite_tables(graph, |mapped| {
            same_table(cassie, mapped, &current).then(|| {
                // Keep the form the definition already uses.
                if mapped.eq_ignore_ascii_case(&current) {
                    next.clone()
                } else {
                    crate::catalog::local_name(&next)
                }
            })
        })
    })
}

/// Follows a column rename in every key, endpoint and property that reads it.
/// Exposed property names are part of the definition and do not change.
pub(super) fn rename_column(
    cassie: &Cassie,
    table: &str,
    from: &str,
    to: &str,
) -> Result<(), QueryError> {
    let canonical = cassie.midge.canonical_collection_name(table);
    let renamed = |column: &mut String| {
        rewrite(column, |current| {
            current.eq_ignore_ascii_case(from).then(|| to.to_owned())
        })
    };
    update_graphs(cassie, |graph| {
        let mut changed = false;
        for vertex in &mut graph.vertices {
            if !same_table(cassie, &vertex.table, &canonical) {
                continue;
            }
            for key in &mut vertex.key {
                changed |= renamed(&mut key.name);
            }
            for property in vertex
                .labels
                .iter_mut()
                .flat_map(|label| &mut label.properties)
            {
                changed |= renamed(&mut property.column);
            }
        }
        for edge in &mut graph.edges {
            if !same_table(cassie, &edge.table, &canonical) {
                continue;
            }
            for key in &mut edge.key {
                changed |= renamed(&mut key.name);
            }
            for column in edge
                .source
                .columns
                .iter_mut()
                .chain(&mut edge.destination.columns)
            {
                changed |= renamed(column);
            }
            for property in edge
                .labels
                .iter_mut()
                .flat_map(|label| &mut label.properties)
            {
                changed |= renamed(&mut property.column);
            }
        }
        changed
    })
}

/// Follows a schema rename: graphs of the schema move with it and mappings of
/// its tables are repointed.
pub(super) fn rename_schema(
    cassie: &Cassie,
    current_schema: &str,
    next_schema: &str,
    relation_renames: &RelationRenames,
) -> Result<(), QueryError> {
    let next_name = |name: &str| {
        Some(rewrite_relation_name_from_map(
            name,
            relation_renames,
            current_schema,
            next_schema,
        ))
    };
    update_graphs(cassie, |graph| {
        let moved = object_in_schema(&graph.name, current_schema)
            && rewrite(&mut graph.name, |name| {
                Some(super::schema_command::schema_namespace_command::rewrite_relation_name_for_schema(
                    name,
                    current_schema,
                    next_schema,
                ))
            });
        rewrite_tables(graph, next_name) || moved
    })
}

/// Whether `schema` holds a property graph.
pub(super) fn schema_has_graphs(cassie: &Cassie, schema: &str) -> bool {
    cassie
        .catalog
        .list_property_graphs()
        .iter()
        .any(|graph| object_in_schema(&graph.name, schema))
}

/// Whether `database` holds a property graph.
pub(super) fn database_has_graphs(cassie: &Cassie, database: &str) -> bool {
    cassie
        .catalog
        .list_property_graphs()
        .iter()
        .any(|graph| crate::catalog::relation_belongs_to_database(&graph.name, database))
}

/// Drops the graphs of a schema that is being dropped with `CASCADE`, after
/// rejecting the drop when a graph of another schema maps one of its tables.
pub(super) fn drop_schema_graphs(cassie: &Cassie, schema: &str) -> Result<(), QueryError> {
    let graphs = cassie.catalog.list_property_graphs();
    let outside = graphs
        .iter()
        .filter(|graph| !object_in_schema(&graph.name, schema))
        .filter(|graph| {
            graph
                .mapped_tables()
                .iter()
                .any(|table| object_in_schema(table, schema))
        })
        .cloned()
        .collect::<Vec<_>>();
    if !outside.is_empty() {
        let subject = format!("schema {}", crate::catalog::local_name(schema));
        return Err(dependency_error(&subject, &outside, true));
    }
    let owned = graphs
        .into_iter()
        .filter(|graph| object_in_schema(&graph.name, schema))
        .collect::<Vec<_>>();
    super::property_graph_views::drop_dependents(cassie, &owned)?;
    for graph in owned {
        cassie
            .midge
            .delete_property_graph(&graph.name)
            .map_err(QueryError::from)?;
        cassie.catalog.unregister_property_graph(&graph.name);
    }
    Ok(())
}

#[cfg(test)]
#[path = "property_graph_dependents_tests.rs"]
mod tests;
