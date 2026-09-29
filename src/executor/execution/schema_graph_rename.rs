use super::{rewrite_relation_name_from_map, Cassie, QueryError, RelationRenames};

pub(super) fn rename_schema_graphs(
    cassie: &Cassie,
    current_schema: &str,
    next_schema: &str,
    relation_renames: &RelationRenames,
) -> Result<(), QueryError> {
    for mut graph in cassie.midge.list_graphs()? {
        let current_name = graph.name.clone();
        let next_name = rewrite_relation_name_from_map(
            &graph.name,
            relation_renames,
            current_schema,
            next_schema,
        );
        let next_node_collection = rewrite_relation_name_from_map(
            &graph.node_collection,
            relation_renames,
            current_schema,
            next_schema,
        );
        let next_edge_collection = rewrite_relation_name_from_map(
            &graph.edge_collection,
            relation_renames,
            current_schema,
            next_schema,
        );
        if current_name == next_name
            && graph.node_collection == next_node_collection
            && graph.edge_collection == next_edge_collection
        {
            continue;
        }
        graph.name = next_name;
        graph.node_collection = next_node_collection;
        graph.edge_collection = next_edge_collection;
        cassie.midge.delete_graph(&current_name)?;
        cassie.midge.put_graph(&graph)?;
    }
    Ok(())
}

/// Repoints graph metadata at a renamed node or edge table, as PostgreSQL
/// keeps dependent objects attached to a relation through `RENAME TO`.
pub(super) fn rename_graph_tables(
    cassie: &Cassie,
    table: &str,
    next_table: &str,
) -> Result<(), QueryError> {
    let current = cassie.midge.canonical_collection_name(table);
    let next = cassie.midge.canonical_collection_name(next_table);
    update_graphs(cassie, |graph| {
        let mut changed = false;
        for collection in [&mut graph.node_collection, &mut graph.edge_collection] {
            if cassie
                .midge
                .canonical_collection_name(collection)
                .eq_ignore_ascii_case(&current)
            {
                collection.clone_from(&next);
                changed = true;
            }
        }
        changed
    })
}

/// Repoints graph metadata at a renamed node or edge column so traversal,
/// edge writes and startup reconciliation keep reading the same field.
pub(super) fn rename_graph_columns(
    cassie: &Cassie,
    table: &str,
    from: &str,
    to: &str,
) -> Result<(), QueryError> {
    update_graphs(cassie, |graph| {
        let mut changed = false;
        for field in graph_fields_on_table(cassie, graph, table) {
            if field.eq_ignore_ascii_case(from) {
                *field = to.to_string();
                changed = true;
            }
        }
        changed
    })
}

/// Refuses to drop a column a graph reads from its node or edge table.
pub(super) fn reject_graph_column_drop(
    cassie: &Cassie,
    table: &str,
    field: &str,
) -> Result<(), QueryError> {
    for mut graph in cassie.midge.list_graphs()? {
        let name = graph.name.clone();
        if graph_fields_on_table(cassie, &mut graph, table)
            .into_iter()
            .any(|column| column.eq_ignore_ascii_case(field))
        {
            return Err(QueryError::General(format!(
                "cannot drop column '{field}' of '{table}' because graph '{name}' depends on it"
            )));
        }
    }
    Ok(())
}

fn graph_fields_on_table<'graph>(
    cassie: &Cassie,
    graph: &'graph mut crate::catalog::GraphMeta,
    table: &str,
) -> Vec<&'graph mut String> {
    let table = cassie.midge.canonical_collection_name(table);
    let is_table = |collection: &str| {
        cassie
            .midge
            .canonical_collection_name(collection)
            .eq_ignore_ascii_case(&table)
    };
    let on_nodes = is_table(&graph.node_collection);
    let on_edges = is_table(&graph.edge_collection);
    let mut fields = Vec::new();
    if on_nodes {
        fields.extend([&mut graph.node_type_field, &mut graph.node_id_field]);
    }
    if on_edges {
        fields.extend([
            &mut graph.edge_id_field,
            &mut graph.source_type_field,
            &mut graph.source_id_field,
            &mut graph.target_type_field,
            &mut graph.target_id_field,
            &mut graph.edge_type_field,
            &mut graph.weight_field,
        ]);
    }
    fields
}

fn update_graphs(
    cassie: &Cassie,
    mut update: impl FnMut(&mut crate::catalog::GraphMeta) -> bool,
) -> Result<(), QueryError> {
    for mut graph in cassie.midge.list_graphs()? {
        if !update(&mut graph) {
            continue;
        }
        cassie.midge.put_graph(&graph)?;
        cassie.catalog.register_graph(graph);
    }
    Ok(())
}
