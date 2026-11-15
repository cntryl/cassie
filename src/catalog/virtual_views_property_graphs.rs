//! Discovery of mapped property graphs (#1255).
//!
//! The `information_schema` views follow the SQL/PGQ names so standard
//! tooling can read a definition. `pg_catalog.pg_property_graph_identities`
//! adds the durable graph and mapping identities those views cannot carry.
//!
//! A graph is listed only for a session that may read every table it maps, so
//! discovery discloses nothing the mapped tables would not.

use super::VirtualRow;
use crate::app::CassieSession;
use crate::catalog::{
    local_name, relation_belongs_to_database, relation_database_name, relation_schema_name,
    Catalog, GraphLabelMeta, PropertyGraphMeta, RoleGrantTarget, RoleObjectPrivilege,
};
use crate::types::{DataType, Value};

const GRAPHS: &str = "information_schema.property_graphs";
const ELEMENT_TABLES: &str = "information_schema.pg_element_tables";
const KEY_COLUMNS: &str = "information_schema.pg_element_table_key_columns";
const EDGE_COMPONENTS: &str = "information_schema.pg_edge_table_components";
const ELEMENT_LABELS: &str = "information_schema.pg_element_table_labels";
const ELEMENT_PROPERTIES: &str = "information_schema.pg_element_table_properties";
const LABEL_PROPERTIES: &str = "information_schema.pg_label_properties";
const IDENTITIES: &str = "pg_catalog.pg_property_graph_identities";

const GRAPH_COLUMNS: [&str; 3] = [
    "property_graph_catalog",
    "property_graph_schema",
    "property_graph_name",
];
const ADMINISTRATOR: &str = "root";

fn columns(own: &[(&str, DataType)]) -> Vec<(String, DataType)> {
    GRAPH_COLUMNS
        .iter()
        .map(|name| ((*name).to_owned(), DataType::Text))
        .chain(
            own.iter()
                .map(|(name, data_type)| ((*name).to_owned(), data_type.clone())),
        )
        .collect()
}

pub(super) fn schema_factory(name: &str) -> Option<super::virtual_views_schema::SchemaFactory> {
    match name {
        GRAPHS => Some(|| columns(&[])),
        ELEMENT_TABLES => Some(|| {
            columns(&[
                ("element_table_alias", DataType::Text),
                ("element_table_kind", DataType::Text),
                ("table_catalog", DataType::Text),
                ("table_schema", DataType::Text),
                ("table_name", DataType::Text),
                ("element_table_definition", DataType::Text),
            ])
        }),
        KEY_COLUMNS => Some(|| {
            columns(&[
                ("element_table_alias", DataType::Text),
                ("column_name", DataType::Text),
                ("ordinal_position", DataType::Int),
            ])
        }),
        EDGE_COMPONENTS => Some(|| {
            columns(&[
                ("element_table_alias", DataType::Text),
                ("vertex_table_alias", DataType::Text),
                ("edge_end", DataType::Text),
                ("edge_table_column_name", DataType::Text),
                ("vertex_table_column_name", DataType::Text),
                ("ordinal_position", DataType::Int),
            ])
        }),
        ELEMENT_LABELS => Some(|| {
            columns(&[
                ("element_table_alias", DataType::Text),
                ("label_name", DataType::Text),
            ])
        }),
        ELEMENT_PROPERTIES => Some(|| {
            columns(&[
                ("element_table_alias", DataType::Text),
                ("property_name", DataType::Text),
                ("property_expression", DataType::Text),
            ])
        }),
        LABEL_PROPERTIES => Some(|| {
            columns(&[
                ("label_name", DataType::Text),
                ("property_name", DataType::Text),
            ])
        }),
        IDENTITIES => Some(|| {
            columns(&[
                ("property_graph_oid", DataType::BigInt),
                ("property_graph_id", DataType::Text),
                ("database_id", DataType::Text),
                ("element_table_alias", DataType::Text),
                ("element_table_kind", DataType::Text),
                ("element_table_id", DataType::Text),
            ])
        }),
        _ => None,
    }
}

fn text(name: &str, value: impl Into<String>) -> (String, Value) {
    (name.to_owned(), Value::String(value.into()))
}

fn position(index: usize) -> (String, Value) {
    let ordinal = i64::try_from(index).map_or(i64::MAX, |index| index.saturating_add(1));
    ("ordinal_position".to_owned(), Value::Int64(ordinal))
}

fn database_of(name: &str, current_database: Option<&str>) -> String {
    relation_database_name(name)
        .or_else(|| current_database.map(str::to_owned))
        .unwrap_or_default()
}

/// One mapping of a graph, vertex or edge, in declaration order.
struct Element<'graph> {
    kind: &'static str,
    alias: &'graph str,
    mapping_id: &'graph str,
    table: &'graph str,
    key: Vec<&'graph str>,
    labels: &'graph [GraphLabelMeta],
}

fn elements(graph: &PropertyGraphMeta) -> Vec<Element<'_>> {
    let vertices = graph.vertices.iter().map(|vertex| Element {
        kind: "VERTEX",
        alias: &vertex.alias,
        mapping_id: &vertex.mapping_id,
        table: &vertex.table,
        key: vertex.key.iter().map(|key| key.name.as_str()).collect(),
        labels: &vertex.labels,
    });
    let edges = graph.edges.iter().map(|edge| Element {
        kind: "EDGE",
        alias: &edge.alias,
        mapping_id: &edge.mapping_id,
        table: &edge.table,
        key: edge.key.iter().map(|key| key.name.as_str()).collect(),
        labels: &edge.labels,
    });
    vertices.chain(edges).collect()
}

/// Whether `session` may read `table`, by the rule ordinary queries follow.
fn may_read(catalog: &Catalog, session: Option<&CassieSession>, table: &str) -> bool {
    let Some(session) = session.filter(|session| session.is_network_authenticated()) else {
        return true;
    };
    let Some(role) = catalog.get_role(&session.user) else {
        return session.user.eq_ignore_ascii_case(ADMINISTRATOR);
    };
    if role.is_admin {
        return true;
    }
    let database = database_of(table, session.current_database());
    let schema = crate::catalog::canonical_schema_name(&database, &relation_schema_name(table));
    let (Some(database), Some(schema), Some(collection)) = (
        catalog.get_database(&database),
        catalog.get_namespace(&schema),
        catalog.get_collection(table),
    ) else {
        return false;
    };
    let granted = |target: RoleGrantTarget, privilege: RoleObjectPrivilege| {
        catalog
            .get_role_object_grant(&database.physical_family, role.object_id, &target)
            .is_some_and(|grant| grant.grants(privilege))
    };
    granted(
        RoleGrantTarget::Schema(schema.object_id),
        RoleObjectPrivilege::Usage,
    ) && granted(
        RoleGrantTarget::Table(collection.storage_id),
        RoleObjectPrivilege::Select,
    )
}

fn element_rows(name: &str, element: &Element<'_>, database: &str) -> Vec<VirtualRow> {
    let alias = || text("element_table_alias", element.alias);
    match name {
        ELEMENT_TABLES => vec![vec![
            alias(),
            text("element_table_kind", element.kind),
            text("table_catalog", database_of(element.table, Some(database))),
            text("table_schema", relation_schema_name(element.table)),
            text("table_name", local_name(element.table)),
            ("element_table_definition".to_owned(), Value::Null),
        ]],
        KEY_COLUMNS => element
            .key
            .iter()
            .enumerate()
            .map(|(index, column)| vec![alias(), text("column_name", *column), position(index)])
            .collect(),
        ELEMENT_LABELS => element
            .labels
            .iter()
            .map(|label| vec![alias(), text("label_name", &label.name)])
            .collect(),
        ELEMENT_PROPERTIES => {
            let mut seen = Vec::<&str>::new();
            element
                .labels
                .iter()
                .flat_map(|label| &label.properties)
                .filter(|property| {
                    let first = !seen.contains(&property.name.as_str());
                    seen.push(&property.name);
                    first
                })
                .map(|property| {
                    vec![
                        alias(),
                        text("property_name", &property.name),
                        text("property_expression", &property.column),
                    ]
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

fn edge_component_rows(graph: &PropertyGraphMeta) -> Vec<VirtualRow> {
    let mut rows = Vec::new();
    for edge in &graph.edges {
        for (end, endpoint) in [("SOURCE", &edge.source), ("DESTINATION", &edge.destination)] {
            let vertex_key = graph
                .vertices
                .iter()
                .find(|vertex| vertex.alias == endpoint.vertex_alias)
                .map(|vertex| vertex.key.as_slice())
                .unwrap_or_default();
            for (index, column) in endpoint.columns.iter().enumerate() {
                let vertex_column = vertex_key.get(index).map_or("", |key| key.name.as_str());
                rows.push(vec![
                    text("element_table_alias", &edge.alias),
                    text("vertex_table_alias", &endpoint.vertex_alias),
                    text("edge_end", end),
                    text("edge_table_column_name", column),
                    text("vertex_table_column_name", vertex_column),
                    position(index),
                ]);
            }
        }
    }
    rows
}

/// Each (label, property) pair of the graph once, in declaration order.
fn label_property_rows(graph: &PropertyGraphMeta) -> Vec<VirtualRow> {
    let mut seen = Vec::<(&str, &str)>::new();
    let mut rows = Vec::new();
    for element in elements(graph) {
        for label in element.labels {
            for property in &label.properties {
                let pair = (label.name.as_str(), property.name.as_str());
                if seen.contains(&pair) {
                    continue;
                }
                seen.push(pair);
                rows.push(vec![
                    text("label_name", &label.name),
                    text("property_name", &property.name),
                ]);
            }
        }
    }
    rows
}

fn graph_rows(name: &str, graph: &PropertyGraphMeta, database: &str) -> Vec<VirtualRow> {
    match name {
        GRAPHS => vec![Vec::new()],
        EDGE_COMPONENTS => edge_component_rows(graph),
        LABEL_PROPERTIES => label_property_rows(graph),
        IDENTITIES => elements(graph)
            .iter()
            .map(|element| {
                vec![
                    (
                        "property_graph_oid".to_owned(),
                        Value::Int64(i64::try_from(graph.object_id).unwrap_or(i64::MAX)),
                    ),
                    text("property_graph_id", &graph.graph_id),
                    text("database_id", &graph.database_id),
                    text("element_table_alias", element.alias),
                    text("element_table_kind", element.kind),
                    text("element_table_id", element.mapping_id),
                ]
            })
            .collect(),
        _ => elements(graph)
            .iter()
            .flat_map(|element| element_rows(name, element, database))
            .collect(),
    }
}

/// The graphs of `current_database` whose every mapped table `session` may read.
pub(crate) fn visible_property_graphs(
    catalog: &Catalog,
    session: Option<&CassieSession>,
    current_database: Option<&str>,
) -> Vec<PropertyGraphMeta> {
    let mut graphs = catalog.list_property_graphs();
    graphs.retain(|graph| {
        current_database.is_none_or(|database| relation_belongs_to_database(&graph.name, database))
            && graph
                .mapped_tables()
                .iter()
                .all(|table| may_read(catalog, session, table))
    });
    graphs
}

pub(super) fn rows(
    catalog: &Catalog,
    name: &str,
    session: Option<&CassieSession>,
    current_database: Option<&str>,
) -> Option<Vec<VirtualRow>> {
    schema_factory(name)?;
    let mut rows = Vec::new();
    for graph in visible_property_graphs(catalog, session, current_database) {
        let database = database_of(&graph.name, current_database);
        let prefix = [
            text(GRAPH_COLUMNS[0], database.clone()),
            text(GRAPH_COLUMNS[1], relation_schema_name(&graph.name)),
            text(GRAPH_COLUMNS[2], local_name(&graph.name)),
        ];
        rows.extend(
            graph_rows(name, &graph, &database)
                .into_iter()
                .map(|own| prefix.iter().cloned().chain(own).collect::<VirtualRow>()),
        );
    }
    Some(rows)
}

#[cfg(test)]
#[path = "virtual_views_property_graphs_tests.rs"]
mod tests;
