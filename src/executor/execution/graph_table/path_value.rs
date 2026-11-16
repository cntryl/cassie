//! The `cassie_graph.graph_path` value of a projected path variable (#1256).
//!
//! A selected match becomes a typed [`Path`] of durable element references, is encoded
//! into the database's reserved record types and passes the shared validator, so an
//! invalid value cannot reach a result.

use std::mem::size_of;

use uuid::Uuid;

use super::bindings::Candidate;
use super::store::Store;
use super::{failure, QueryError, SourceExecutionEnv};
use crate::catalog::PropertyGraphMeta;
use crate::executor::retained_memory::{add, mul};
use crate::types::graph::{Direction, ElementKind, ElementRef, GraphDescriptors, KeyPart, Path};
use crate::types::typed::TypedValue;
use crate::types::Value;

/// Builds the path values of one `GRAPH_TABLE` source.
pub(super) struct PathBuilder {
    descriptors: GraphDescriptors,
    database_id: Uuid,
    graph_id: Uuid,
}

fn identity(text: &str) -> Result<Uuid, QueryError> {
    Uuid::parse_str(text).map_err(|_| failure("the property graph has no durable identity"))
}

impl PathBuilder {
    /// # Errors
    ///
    /// `42704` while the reserved schema is not bootstrapped, or an execution error when
    /// the graph has no durable identity.
    pub(super) fn new(
        env: &SourceExecutionEnv<'_>,
        graph: &PropertyGraphMeta,
    ) -> Result<Self, QueryError> {
        Ok(Self {
            descriptors: crate::executor::filter::graph_functions::descriptors_of(env.session)?,
            database_id: identity(&graph.database_id)?,
            graph_id: identity(&graph.graph_id)?,
        })
    }

    fn element(
        &self,
        kind: ElementKind,
        (mapping, key): (&str, &[super::keys::KeyPart]),
    ) -> Result<ElementRef, QueryError> {
        Ok(ElementRef {
            database_id: self.database_id,
            graph_id: self.graph_id,
            mapping_id: identity(mapping)?,
            kind,
            key: key.iter().map(super::keys::KeyPart::to_model).collect(),
        })
    }

    /// The structural path of `candidate`: vertices, incoming edges and directions in path
    /// order, 1 for source to destination and 2 for the reverse.
    fn path(&self, store: &Store, candidate: &Candidate) -> Result<Path, QueryError> {
        let vertex = |id: &usize| {
            let (mapping, key) = store.vertex_key(*id);
            self.element(ElementKind::Vertex, (mapping, key))
        };
        let edge = |id: &usize| {
            let (mapping, key) = store.edge_key(*id);
            self.element(ElementKind::Edge, (mapping, key))
        };
        let directions = candidate
            .directions
            .iter()
            .map(|direction| match direction {
                1 => Ok(Direction::Forward),
                2 => Ok(Direction::Reverse),
                _ => Err(failure("a path step has an unknown direction")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Path {
            vertices: candidate
                .vertices
                .iter()
                .map(vertex)
                .collect::<Result<_, _>>()?,
            edges: candidate.edges.iter().map(edge).collect::<Result<_, _>>()?,
            directions,
        })
    }

    /// Bytes the detached structural path of `candidate` holds while it is encoded.
    fn scratch_bytes(store: &Store, candidate: &Candidate) -> Result<usize, QueryError> {
        let keys = candidate
            .vertices
            .iter()
            .map(|id| store.vertex_key(*id).1)
            .chain(candidate.edges.iter().map(|id| store.edge_key(*id).1));
        let mut bytes = mul(
            candidate.vertices.len() + candidate.edges.len(),
            size_of::<ElementRef>(),
        )?;
        for key in keys {
            let text = key.iter().map(super::keys::KeyPart::heap_bytes).sum();
            bytes = add(bytes, add(mul(key.len(), size_of::<KeyPart>())?, text)?)?;
        }
        add(bytes, candidate.directions.len()).map_err(Into::into)
    }

    /// The `graph_path` record of `candidate`. The transient structural path is reserved
    /// against the query budget while it is encoded; the caller accounts the record.
    ///
    /// # Errors
    ///
    /// The memory budget, cancellation, and `22023` when the validator rejects the value.
    pub(super) fn value(
        &self,
        env: &SourceExecutionEnv<'_>,
        store: &Store,
        candidate: &Candidate,
    ) -> Result<Value, QueryError> {
        let _scratch = env
            .controls
            .reserve_query_memory(Self::scratch_bytes(store, candidate)?)?;
        let path = self.path(store, candidate)?;
        let value = path
            .to_value(&self.descriptors)
            .map_err(|message| failure(&message))?;
        if let Value::Typed(TypedValue::Record(record)) = &value {
            crate::executor::filter::graph_validation::validate(record)?;
        }
        Ok(value)
    }
}
