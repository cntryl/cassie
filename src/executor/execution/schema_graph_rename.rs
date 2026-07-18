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
