//! Resolution of graph, mapping and element references (#1268).
//!
//! References carry durable UUIDs, not names or type OIDs: a rename keeps them
//! resolvable, while a dropped and recreated graph has new identities and an
//! old reference stops resolving (`42704`) instead of retargeting.
use uuid::Uuid;

use super::property_graphs::{GraphKeyColumn, PropertyGraphMeta};
use super::Catalog;
use crate::app::{CassieError, DiagnosticError};
use crate::types::graph::{ElementKind, ElementRef, KeyPart};
use crate::types::DataType;

/// One vertex or edge mapping of a property graph, with its durable identities.
#[derive(Debug, Clone)]
pub struct ResolvedMapping {
    pub graph_name: String,
    pub database_id: Uuid,
    pub graph_id: Uuid,
    pub mapping_id: Uuid,
    pub kind: ElementKind,
    /// Canonical name of the mapped table.
    pub table: String,
    pub key: Vec<GraphKeyColumn>,
}

fn undefined(message: String) -> CassieError {
    DiagnosticError::new("42704", message).into()
}

fn parse(identity: &str, what: &str) -> Result<Uuid, CassieError> {
    Uuid::parse_str(identity)
        .map_err(|_| undefined(format!("{what} identity is not available")))
}

/// The `type_tag` a key column of `data_type` carries.
fn column_tag(data_type: &DataType) -> Option<i16> {
    match data_type.flattened() {
        DataType::SmallInt => Some(1),
        DataType::Int => Some(2),
        DataType::BigInt => Some(3),
        DataType::Text => Some(4),
        DataType::Uuid => Some(5),
        _ => None,
    }
}

impl ResolvedMapping {
    /// The mapping `alias` of `kind` in `graph`.
    ///
    /// # Errors
    /// Returns `42704` when the graph has no such mapping.
    pub fn by_alias(
        graph: &PropertyGraphMeta,
        alias: &str,
        kind: ElementKind,
    ) -> Result<Self, CassieError> {
        let found = match kind {
            ElementKind::Vertex => graph
                .vertices
                .iter()
                .find(|mapping| mapping.alias.eq_ignore_ascii_case(alias))
                .map(|mapping| (&mapping.mapping_id, &mapping.table, &mapping.key)),
            ElementKind::Edge => graph
                .edges
                .iter()
                .find(|mapping| mapping.alias.eq_ignore_ascii_case(alias))
                .map(|mapping| (&mapping.mapping_id, &mapping.table, &mapping.key)),
        };
        let (mapping_id, table, key) = found.ok_or_else(|| {
            undefined(format!(
                "{} mapping \"{alias}\" does not exist in property graph \"{}\"",
                kind.name(),
                crate::catalog::local_name(&graph.name)
            ))
        })?;
        Self::from_parts(graph, kind, mapping_id, table, key)
    }

    fn from_parts(
        graph: &PropertyGraphMeta,
        kind: ElementKind,
        mapping_id: &str,
        table: &str,
        key: &[GraphKeyColumn],
    ) -> Result<Self, CassieError> {
        Ok(Self {
            graph_name: graph.name.clone(),
            database_id: parse(&graph.database_id, "database")?,
            graph_id: parse(&graph.graph_id, "graph")?,
            mapping_id: parse(mapping_id, "mapping")?,
            kind,
            table: table.to_owned(),
            key: key.to_vec(),
        })
    }

    /// Checks key arity and exact SQL types against the mapped key columns.
    ///
    /// # Errors
    /// Returns `42804` for a wrong number of components or a component whose
    /// type is not the declared key column type.
    pub fn check_key(&self, key: &[KeyPart]) -> Result<(), CassieError> {
        let mismatch = |detail: String| -> CassieError {
            DiagnosticError::new("42804", detail).into()
        };
        if key.len() != self.key.len() {
            return Err(mismatch(format!(
                "the {} key has {} columns but {} key parts were supplied",
                self.kind.name(),
                self.key.len(),
                key.len()
            )));
        }
        for (part, column) in key.iter().zip(&self.key) {
            if column_tag(&column.data_type) != Some(part.type_tag()) {
                return Err(mismatch(format!(
                    "key part for column \"{}\" must be {}, not {}",
                    column.name,
                    column.data_type.type_name(),
                    part.type_name()
                )));
            }
        }
        Ok(())
    }

    /// The reference to the element of this mapping with `key`.
    #[must_use]
    pub fn element(&self, key: Vec<KeyPart>) -> ElementRef {
        ElementRef {
            database_id: self.database_id,
            graph_id: self.graph_id,
            mapping_id: self.mapping_id,
            kind: self.kind,
            key,
        }
    }
}

impl Catalog {
    /// Resolves the graph and mapping `reference` names by durable identity.
    ///
    /// # Errors
    /// Returns `42704` when the graph or mapping no longer exists (including a
    /// dropped and recreated graph), belongs to another database, or the
    /// reference's kind or key shape no longer matches the mapping.
    pub fn resolve_element_ref(
        &self,
        reference: &ElementRef,
        current_database: &str,
    ) -> Result<ResolvedMapping, CassieError> {
        let graph = self
            .list_property_graphs()
            .into_iter()
            .find(|graph| parse(&graph.graph_id, "graph").is_ok_and(|id| id == reference.graph_id))
            .filter(|graph| {
                parse(&graph.database_id, "database").is_ok_and(|id| id == reference.database_id)
                    && crate::catalog::relation_belongs_to_database(&graph.name, current_database)
            })
            .ok_or_else(|| {
                undefined("the property graph of this reference does not exist".to_owned())
            })?;
        let mapping = match reference.kind {
            ElementKind::Vertex => graph
                .vertices
                .iter()
                .find(|mapping| parse(&mapping.mapping_id, "mapping").is_ok_and(|id| id == reference.mapping_id))
                .map(|mapping| (&mapping.mapping_id, &mapping.table, &mapping.key)),
            ElementKind::Edge => graph
                .edges
                .iter()
                .find(|mapping| parse(&mapping.mapping_id, "mapping").is_ok_and(|id| id == reference.mapping_id))
                .map(|mapping| (&mapping.mapping_id, &mapping.table, &mapping.key)),
        }
        .ok_or_else(|| undefined("the mapping of this reference does not exist".to_owned()))?;
        let resolved = ResolvedMapping::from_parts(&graph, reference.kind, mapping.0, mapping.1, mapping.2)?;
        resolved
            .check_key(&reference.key)
            .map_err(|_| undefined("the key of this reference no longer matches its mapping".to_owned()))?;
        Ok(resolved)
    }
}
