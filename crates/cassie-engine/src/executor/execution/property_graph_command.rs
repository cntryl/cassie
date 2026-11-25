//! `CREATE`, `DROP` and `ALTER PROPERTY GRAPH` (#1255).
//!
//! A definition is validated against the catalog and stored. It builds no
//! derived state: graph reads go to the mapped tables, through their ordinary
//! indexes where present. Dropping a
//! graph never touches the tables it maps or their rows.

use super::command_output::empty as empty_command;
use super::{Cassie, CassieSession, QueryError, QueryResult};
use crate::app::DiagnosticError;
use crate::catalog::{
    GraphEdgeMeta, GraphEndpointMeta, GraphKeyColumn, GraphLabelMeta, GraphPropertyMeta,
    GraphVertexMeta, PropertyGraphMeta,
};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    CreatePropertyGraph, GraphEndpoint, GraphLabel, IdentifierPath, PropertyGraphAction,
    PropertyGraphStatement,
};
use crate::sql::binder::BindingContext;
use crate::types::DataType;

const DUPLICATE_OBJECT: &str = "42710";
const UNDEFINED_COLUMN: &str = "42703";
const UNDEFINED_OBJECT: &str = "42704";
const INVALID_KEY: &str = "42830";
const DATATYPE_MISMATCH: &str = "42804";
const UNSUPPORTED: &str = "0A000";

fn diagnostic(sql_state: &'static str, message: String) -> QueryError {
    QueryError::Cassie(DiagnosticError::new(sql_state, message).into())
}

pub(super) fn execute(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    statement: &PropertyGraphStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let context = cassie.binding_context_for_session(session);
    let output = empty_command(statement.action.command_tag(), controls)?;
    match &statement.action {
        PropertyGraphAction::Create(graph) => create(cassie, &context, graph)?,
        PropertyGraphAction::Drop { name, if_exists } => {
            drop_graph(cassie, &context, name, *if_exists)?;
        }
        PropertyGraphAction::Rename { name, new_name } => {
            rename(cassie, &context, name, new_name)?;
        }
    }
    Ok(output)
}

/// The canonical name of an existing graph, resolved through `search_path`.
fn existing_graph(
    cassie: &Cassie,
    context: &BindingContext,
    name: &IdentifierPath,
) -> Result<Option<PropertyGraphMeta>, QueryError> {
    let canonical =
        crate::sql::binder::resolve_existing_name(name.as_str(), context, |candidate| {
            cassie.catalog.get_property_graph(candidate).is_some()
        })
        .map_err(QueryError::from)?;
    Ok(cassie.catalog.get_property_graph(&canonical))
}

fn missing_graph(name: &IdentifierPath) -> QueryError {
    diagnostic(
        UNDEFINED_OBJECT,
        format!("property graph \"{}\" does not exist", name.as_str()),
    )
}

fn drop_graph(
    cassie: &Cassie,
    context: &BindingContext,
    name: &IdentifierPath,
    if_exists: bool,
) -> Result<(), QueryError> {
    let Some(graph) = existing_graph(cassie, context, name)? else {
        return if if_exists {
            Ok(())
        } else {
            Err(missing_graph(name))
        };
    };
    super::property_graph_views::reject_drop(cassie, context, &graph)?;
    cassie
        .midge
        .delete_property_graph(&graph.name)
        .map_err(QueryError::from)?;
    cassie.catalog.unregister_property_graph(&graph.name);
    Ok(())
}

fn rename(
    cassie: &Cassie,
    context: &BindingContext,
    name: &IdentifierPath,
    new_name: &str,
) -> Result<(), QueryError> {
    let Some(graph) = existing_graph(cassie, context, name)? else {
        return Err(missing_graph(name));
    };
    super::property_graph_views::reject_rename(cassie, context, &graph)?;
    // A rename stays in the graph's schema and keeps every identity.
    let renamed_name = crate::catalog::derive_scoped_name(&graph.name, |_| new_name.to_owned());
    if cassie.catalog.get_property_graph(&renamed_name).is_some() {
        return Err(diagnostic(
            DUPLICATE_OBJECT,
            format!("property graph \"{new_name}\" already exists"),
        ));
    }
    let renamed = PropertyGraphMeta {
        name: renamed_name,
        ..graph.clone()
    };
    cassie
        .midge
        .rename_property_graph(&graph.name, &renamed)
        .map_err(QueryError::from)?;
    cassie.catalog.unregister_property_graph(&graph.name);
    cassie.catalog.register_property_graph(renamed);
    Ok(())
}

fn create(
    cassie: &Cassie,
    context: &BindingContext,
    graph: &CreatePropertyGraph,
) -> Result<(), QueryError> {
    // The graph feature is published only with its reserved types: a user-owned
    // `cassie_graph` schema fails here with `42710` until the operator resolves it.
    cassie
        .ensure_graph_schema(&context.database)
        .map_err(QueryError::from)?;
    let name =
        crate::sql::binder::normalize_new_relation_path(&graph.name, context, &cassie.catalog)
            .map_err(QueryError::from)?;
    if cassie.catalog.get_property_graph(&name).is_some() {
        return Err(diagnostic(
            DUPLICATE_OBJECT,
            format!("property graph \"{}\" already exists", graph.name.as_str()),
        ));
    }
    let mut vertices = Vec::<GraphVertexMeta>::with_capacity(graph.vertices.len());
    for mapping in &graph.vertices {
        reject_duplicate_alias(vertices.iter().map(|vertex| &vertex.alias), &mapping.alias)?;
        let source = Source::resolve(cassie, context, &mapping.table)?;
        vertices.push(GraphVertexMeta {
            mapping_id: uuid::Uuid::new_v4().to_string(),
            alias: mapping.alias.clone(),
            key: source.key(&mapping.alias, &mapping.key)?,
            labels: source.labels(&mapping.alias, &mapping.labels)?,
            table: source.table,
        });
    }
    let mut edges = Vec::<GraphEdgeMeta>::with_capacity(graph.edges.len());
    for mapping in &graph.edges {
        reject_duplicate_alias(edges.iter().map(|edge| &edge.alias), &mapping.alias)?;
        let source = Source::resolve(cassie, context, &mapping.table)?;
        edges.push(GraphEdgeMeta {
            mapping_id: uuid::Uuid::new_v4().to_string(),
            alias: mapping.alias.clone(),
            key: source.key(&mapping.alias, &mapping.key)?,
            source: source.endpoint(&mapping.alias, "SOURCE", &mapping.source, &vertices)?,
            destination: source.endpoint(
                &mapping.alias,
                "DESTINATION",
                &mapping.destination,
                &vertices,
            )?,
            labels: source.labels(&mapping.alias, &mapping.labels)?,
            table: source.table,
        });
    }
    let stored = cassie
        .midge
        .put_property_graph(&PropertyGraphMeta {
            name,
            object_id: 0,
            graph_id: uuid::Uuid::new_v4().to_string(),
            database_id: String::new(),
            vertices,
            edges,
        })
        .map_err(QueryError::from)?;
    cassie.catalog.register_property_graph(stored);
    Ok(())
}

fn reject_duplicate_alias<'a>(
    mut existing: impl Iterator<Item = &'a String>,
    alias: &str,
) -> Result<(), QueryError> {
    if existing.any(|other| other == alias) {
        return Err(diagnostic(
            DUPLICATE_OBJECT,
            format!("property graph mapping alias \"{alias}\" is declared more than once"),
        ));
    }
    Ok(())
}

/// One mapped table and the catalog facts its mapping is checked against.
pub(super) struct Source {
    table: String,
    columns: Vec<(String, DataType)>,
    /// Columns declared with a domain; a domain is not an eligible key type.
    domain_columns: Vec<String>,
    constraints: Vec<crate::catalog::FieldConstraint>,
    indexes: Vec<crate::catalog::IndexMeta>,
}

impl Source {
    /// The catalog facts of an existing table, by canonical name.
    pub(super) fn of_table(cassie: &Cassie, canonical: &str) -> Option<Self> {
        let schema = cassie.catalog.get_schema(canonical)?;
        Some(Self {
            domain_columns: schema
                .fields
                .iter()
                .filter(|field| field.domain.is_some())
                .map(|field| field.name.clone())
                .collect(),
            columns: schema
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
            constraints: cassie.catalog.get_constraints(&schema.collection),
            indexes: cassie.catalog.list_indexes(&schema.collection),
            table: schema.collection,
        })
    }

    pub(super) fn replace_constraints(
        &mut self,
        constraints: Vec<crate::catalog::FieldConstraint>,
    ) {
        self.constraints = constraints;
    }

    pub(super) fn forget_indexes(&mut self, names: &[&str]) {
        self.indexes.retain(|index| {
            !names
                .iter()
                .any(|name| index.name.eq_ignore_ascii_case(name))
        });
    }

    fn resolve(
        cassie: &Cassie,
        context: &BindingContext,
        table: &IdentifierPath,
    ) -> Result<Self, QueryError> {
        let canonical = crate::sql::binder::resolve_relation_path(table, &cassie.catalog, context)
            .map_err(QueryError::from)?;
        if cassie.catalog.get_view(&canonical).is_some() {
            return Err(diagnostic(
                UNSUPPORTED,
                format!(
                    "property graphs can map only tables; \"{}\" is a view",
                    table.as_str()
                ),
            ));
        }
        // A derived relation is dropped and rebuilt by its owner, outside the
        // table lifecycle a graph can guard.
        let materialized = cassie
            .catalog
            .get_projection_metadata(&canonical)
            .is_some_and(|projection| {
                projection.kind == crate::catalog::ProjectionKind::Materialized
            });
        let rollup_output = cassie
            .catalog
            .list_rollups()
            .iter()
            .any(|rollup| rollup.output_collection.eq_ignore_ascii_case(&canonical));
        if materialized || rollup_output {
            return Err(diagnostic(
                UNSUPPORTED,
                format!(
                    "property graphs can map only tables; \"{}\" is a derived relation",
                    table.as_str()
                ),
            ));
        }
        Self::of_table(cassie, &canonical).ok_or_else(|| {
            QueryError::Cassie(crate::app::CassieError::CatalogObjectNotFound {
                kind: crate::app::CatalogObjectKind::Relation,
                name: table.as_str().to_owned(),
            })
        })
    }

    fn column(&self, alias: &str, name: &str) -> Result<(String, DataType), QueryError> {
        self.columns
            .iter()
            .find(|(column, _)| {
                crate::sql::ColumnIdentifierPath::matches_stored_field(name, column)
            })
            .cloned()
            .ok_or_else(|| {
                diagnostic(
                    UNDEFINED_COLUMN,
                    format!("column \"{name}\" of the table mapped as \"{alias}\" does not exist"),
                )
            })
    }

    /// The ordered key tuple, checked for a supported type, a NOT NULL
    /// declaration and a non-partial unique key on exactly these columns.
    fn key(&self, alias: &str, names: &[String]) -> Result<Vec<GraphKeyColumn>, QueryError> {
        let mut key = Vec::with_capacity(names.len());
        for name in names {
            let (name, data_type) = self.column(alias, name)?;
            let eligible = matches!(
                data_type,
                DataType::SmallInt
                    | DataType::Int
                    | DataType::BigInt
                    | DataType::Text
                    | DataType::Uuid
            ) && !self.domain_columns.contains(&name);
            if !eligible {
                return Err(diagnostic(
                    UNSUPPORTED,
                    format!(
                        "key column \"{name}\" of mapping \"{alias}\" has type {}; property graph \
                         keys must be smallint, integer, bigint, text or uuid",
                        data_type.type_name()
                    ),
                ));
            }
            if !self.is_not_null(&name) {
                return Err(diagnostic(
                    INVALID_KEY,
                    format!("key column \"{name}\" of mapping \"{alias}\" must be NOT NULL"),
                ));
            }
            key.push(GraphKeyColumn { name, data_type });
        }
        if !self.has_unique_key(&key) {
            return Err(diagnostic(
                INVALID_KEY,
                format!(
                    "mapping \"{alias}\" needs a primary key, unique constraint or non-partial \
                     unique index on exactly ({})",
                    names.join(", ")
                ),
            ));
        }
        Ok(key)
    }

    pub(super) fn is_not_null(&self, column: &str) -> bool {
        self.constraints.iter().any(|constraint| {
            constraint.field == column && (constraint.not_null || constraint.primary_key)
        })
    }

    pub(super) fn has_unique_key(&self, key: &[GraphKeyColumn]) -> bool {
        let wanted = key
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>();
        let same = |fields: &[&str]| {
            fields.len() == wanted.len()
                && fields
                    .iter()
                    .zip(&wanted)
                    .all(|(left, right)| left == right)
        };
        let ordered =
            |ordinal: fn(&crate::catalog::FieldConstraint) -> Option<u32>,
             group: &dyn Fn(&crate::catalog::FieldConstraint) -> bool| {
                let mut members = self
                    .constraints
                    .iter()
                    .filter(|constraint| group(constraint))
                    .filter_map(|constraint| {
                        ordinal(constraint).map(|at| (at, constraint.field.as_str()))
                    })
                    .collect::<Vec<_>>();
                members.sort_unstable_by_key(|(at, _)| *at);
                members
                    .into_iter()
                    .map(|(_, field)| field)
                    .collect::<Vec<_>>()
            };
        let primary = ordered(|constraint| constraint.primary_key_ordinal, &|constraint| {
            constraint.primary_key
        });
        if !primary.is_empty() && same(&primary) {
            return true;
        }
        let mut unique_names = self
            .constraints
            .iter()
            .filter_map(|constraint| constraint.unique_name.as_deref())
            .collect::<Vec<_>>();
        unique_names.sort_unstable();
        unique_names.dedup();
        if unique_names.into_iter().any(|name| {
            same(&ordered(
                |constraint| constraint.unique_ordinal,
                &|constraint| constraint.unique && constraint.unique_name.as_deref() == Some(name),
            ))
        }) {
            return true;
        }
        if let [only] = wanted.as_slice() {
            if self.constraints.iter().any(|constraint| {
                constraint.unique && constraint.unique_name.is_none() && constraint.field == *only
            }) {
                return true;
            }
        }
        self.indexes.iter().any(|index| {
            let fields = if index.fields.is_empty() {
                vec![index.field.as_str()]
            } else {
                index.fields.iter().map(String::as_str).collect()
            };
            index.unique
                && index.kind == crate::catalog::IndexKind::Scalar
                && index.predicate.is_none()
                && index.expressions.is_empty()
                && same(&fields)
        })
    }

    fn labels(
        &self,
        alias: &str,
        labels: &[GraphLabel],
    ) -> Result<Vec<GraphLabelMeta>, QueryError> {
        let mut out = Vec::<GraphLabelMeta>::with_capacity(labels.len());
        for label in labels {
            if out.iter().any(|existing| existing.name == label.name) {
                return Err(diagnostic(
                    DUPLICATE_OBJECT,
                    format!(
                        "label \"{}\" is declared more than once on mapping \"{alias}\"",
                        label.name
                    ),
                ));
            }
            let mut properties = Vec::<GraphPropertyMeta>::with_capacity(label.properties.len());
            for property in &label.properties {
                if properties
                    .iter()
                    .any(|existing| existing.name == property.name)
                {
                    return Err(diagnostic(
                        DUPLICATE_OBJECT,
                        format!(
                            "property \"{}\" is declared more than once in label \"{}\"",
                            property.name, label.name
                        ),
                    ));
                }
                let (column, data_type) = self.column(alias, &property.column)?;
                properties.push(GraphPropertyMeta {
                    name: property.name.clone(),
                    column,
                    data_type,
                });
            }
            out.push(GraphLabelMeta {
                name: label.name.clone(),
                properties,
            });
        }
        Ok(out)
    }

    /// One endpoint: its columns must match the referenced vertex key in
    /// arity, order and exact SQL type.
    fn endpoint(
        &self,
        alias: &str,
        side: &str,
        endpoint: &GraphEndpoint,
        vertices: &[GraphVertexMeta],
    ) -> Result<GraphEndpointMeta, QueryError> {
        let vertex = vertices
            .iter()
            .find(|vertex| vertex.alias == endpoint.vertex_alias)
            .ok_or_else(|| {
                diagnostic(
                    UNDEFINED_OBJECT,
                    format!(
                        "{side} of edge mapping \"{alias}\" references unknown vertex mapping \"{}\"",
                        endpoint.vertex_alias
                    ),
                )
            })?;
        let references_key = endpoint.references.len() == vertex.key.len()
            && endpoint
                .references
                .iter()
                .zip(&vertex.key)
                .all(|(reference, key)| {
                    crate::sql::ColumnIdentifierPath::matches_stored_field(reference, &key.name)
                });
        if !references_key || endpoint.columns.len() != vertex.key.len() {
            return Err(diagnostic(
                INVALID_KEY,
                format!(
                    "{side} of edge mapping \"{alias}\" must reference the KEY of vertex mapping \
                     \"{}\" with the same number of columns in the same order",
                    vertex.alias
                ),
            ));
        }
        let mut columns = Vec::with_capacity(endpoint.columns.len());
        for (name, key) in endpoint.columns.iter().zip(&vertex.key) {
            let (name, data_type) = self.column(alias, name)?;
            if data_type != key.data_type {
                return Err(diagnostic(
                    DATATYPE_MISMATCH,
                    format!(
                        "{side} column \"{name}\" of edge mapping \"{alias}\" has type {} but \
                         vertex key \"{}\" has type {}",
                        data_type.type_name(),
                        key.name,
                        key.data_type.type_name()
                    ),
                ));
            }
            columns.push(name);
        }
        Ok(GraphEndpointMeta {
            columns,
            vertex_alias: vertex.alias.clone(),
        })
    }
}

#[cfg(test)]
#[path = "property_graph_command_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "property_graph_durability_tests.rs"]
mod durability_tests;
