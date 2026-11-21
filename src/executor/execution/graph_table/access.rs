//! The access path of each mapping a `GRAPH_TABLE` reads: indexed expansion or the exact
//! full read, and how the start vertices are found. The decision is made per statement from
//! the catalog and the pattern, and is the one `EXPLAIN` reports.

use super::keys::{key_part, KeyPart};
use super::plan::{Columns, Plan};
use crate::app::{Cassie, CassieSession};
use crate::catalog::{GraphKeyColumn, IndexMeta, PropertyGraphMeta};
use crate::sql::ast::{BinaryOp, Expr, GraphEdgeDirection};
use crate::types::Value;

/// Why a mapping is read exactly.
pub(super) const FORCED_EXACT: &str = "exact_reads_forced";
pub(super) const NO_ENDPOINT_INDEX: &str = "no_endpoint_index";
pub(super) const NO_KEY_INDEX: &str = "no_key_index";

/// A plain scalar index whose leading columns cover the looked-up columns.
#[derive(Clone, Debug)]
pub(super) struct KeyIndex {
    pub(super) index: IndexMeta,
    /// For each index field, the position of its column in the looked-up column list.
    pub(super) order: Vec<usize>,
}

impl KeyIndex {
    /// The equality prefix of `key` (given in looked-up column order) in index order.
    pub(super) fn probe(&self, key: &[KeyPart]) -> Vec<serde_json::Value> {
        self.order
            .iter()
            .map(|position| key_json(&key[*position]))
            .collect()
    }
}

fn key_json(part: &KeyPart) -> serde_json::Value {
    match part {
        KeyPart::SmallInt(value) => serde_json::Value::from(*value),
        KeyPart::Integer(value) => serde_json::Value::from(*value),
        KeyPart::BigInt(value) => serde_json::Value::from(*value),
        KeyPart::Text(value) => serde_json::Value::String(value.clone()),
        KeyPart::Uuid(bytes) => {
            serde_json::Value::String(uuid::Uuid::from_bytes(*bytes).hyphenated().to_string())
        }
    }
}

/// The scalar index of `table` on exactly `columns`, in any column order, usable by the
/// session's snapshot. See [`crate::catalog::graph_key_index::scalar_key_index`].
pub(super) fn find_index(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    table: &str,
    columns: &[String],
) -> Option<KeyIndex> {
    crate::catalog::graph_key_index::scalar_key_index(&cassie.catalog, session, table, columns)
        .map(|(index, order)| KeyIndex { index, order })
}

/// Which endpoint columns of an edge mapping a lookup uses.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum Side {
    Source,
    Destination,
}

pub(super) struct EdgeAccess {
    pub(super) source: Option<KeyIndex>,
    pub(super) destination: Option<KeyIndex>,
    pub(super) reason: Option<&'static str>,
}

impl EdgeAccess {
    pub(super) fn indexed(&self) -> bool {
        self.reason.is_none()
    }

    pub(super) fn index(&self, side: Side) -> Option<&KeyIndex> {
        match side {
            Side::Source => self.source.as_ref(),
            Side::Destination => self.destination.as_ref(),
        }
    }
}

pub(super) struct Access {
    /// By vertex mapping: the key index, `None` when the mapping is not read lazily.
    pub(super) vertex: Vec<Option<KeyIndex>>,
    pub(super) vertex_read: Vec<bool>,
    pub(super) edge: Vec<Option<EdgeAccess>>,
    pub(super) forced_exact: bool,
}

fn key_columns(key: &[GraphKeyColumn]) -> Vec<String> {
    key.iter().map(|column| column.name.clone()).collect()
}

impl Access {
    /// The access paths covering every pattern of a table: a mapping is read when any
    /// pattern reads it, and an edge side is looked up when any pattern's step needs it.
    pub(super) fn choose_all(
        cassie: &Cassie,
        session: Option<&CassieSession>,
        plans: &[Plan<'_>],
    ) -> Self {
        let needs = |mapping: usize| {
            let mut needs = [false; 2];
            for step in plans.iter().flat_map(|plan| &plan.steps) {
                if step.selection[mapping].is_none() {
                    continue;
                }
                match step.direction {
                    GraphEdgeDirection::Outgoing => needs[0] = true,
                    GraphEdgeDirection::Incoming => needs[1] = true,
                    GraphEdgeDirection::Undirected => needs = [true, true],
                }
            }
            needs
        };
        let (vertices, edges) = super::plan::mappings_read(plans);
        Self::for_mappings(
            (cassie, session),
            &plans[0].graph,
            (&vertices, &edges),
            needs,
        )
    }

    /// The access paths for explicit vertex and edge mapping lists; `needs` tells, for an
    /// edge mapping, whether its source side and its destination side are looked up.
    pub(super) fn for_mappings(
        (cassie, session): (&Cassie, Option<&CassieSession>),
        graph: &PropertyGraphMeta,
        (vertex_list, edge_list): (&[usize], &[usize]),
        needs: impl Fn(usize) -> [bool; 2],
    ) -> Self {
        let forced_exact = cassie.runtime.graph_exact_reads();
        let mut vertex = vec![None; graph.vertices.len()];
        let mut vertex_read = vec![false; graph.vertices.len()];
        for &mapping in vertex_list {
            vertex_read[mapping] = true;
            if !forced_exact {
                let meta = &graph.vertices[mapping];
                vertex[mapping] = find_index(cassie, session, &meta.table, &key_columns(&meta.key));
            }
        }
        let mut edge = (0..graph.edges.len()).map(|_| None).collect::<Vec<_>>();
        for &mapping in edge_list {
            let meta = &graph.edges[mapping];
            let sides = needs(mapping);
            let find = |wanted: bool, columns: &[String]| {
                if forced_exact || !wanted {
                    return None;
                }
                find_index(cassie, session, &meta.table, columns)
            };
            let source = find(sides[0], &meta.source.columns);
            let destination = find(sides[1], &meta.destination.columns);
            let reason = if forced_exact {
                Some(FORCED_EXACT)
            } else if (sides[0] && source.is_none()) || (sides[1] && destination.is_none()) {
                Some(NO_ENDPOINT_INDEX)
            } else {
                None
            };
            edge[mapping] = Some(EdgeAccess {
                source,
                destination,
                reason,
            });
        }
        Self {
            vertex,
            vertex_read,
            edge,
            forced_exact,
        }
    }
}

/// The equalities of an inline predicate that is exactly a conjunction of
/// `property = constant or parameter` terms, as `(property entry, constant)`.
fn equalities<'a>(expr: &'a Expr, out: &mut Vec<(&'a str, &'a Expr)>) -> bool {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => equalities(left, out) && equalities(right, out),
        Expr::Binary {
            left,
            op: BinaryOp::Eq,
            right,
        } => match (left.as_ref(), right.as_ref()) {
            (Expr::Column(name), constant) | (constant, Expr::Column(name))
                if is_constant(constant) =>
            {
                out.push((name.as_str(), constant));
                true
            }
            _ => false,
        },
        _ => false,
    }
}

const fn is_constant(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Param(_) | Expr::IntegerLiteral(_) | Expr::StringLiteral(_)
    )
}

/// For a vertex mapping selected at the start position, the constants that fix its complete
/// key in key order, when the start pattern's inline predicate is a pure conjunction of
/// equalities covering every key column once; `None` reads the mapping exactly.
pub(super) fn start_anchor<'a>(
    plan: &Plan<'a>,
    meta_key: &[GraphKeyColumn],
    columns: &Columns,
) -> Option<Vec<&'a Expr>> {
    let filter = plan.positions.first()?.filter.as_ref()?;
    let mut terms = Vec::new();
    if !equalities(filter.expr, &mut terms) {
        return None;
    }
    let column_of = |entry: &str| {
        columns
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(entry))
            .map(|(_, column)| column.as_str())
    };
    let mut constants = Vec::with_capacity(meta_key.len());
    for key in meta_key {
        let mut matching = terms.iter().filter(|(entry, _)| {
            column_of(entry).is_some_and(|column| column.eq_ignore_ascii_case(&key.name))
        });
        let (_, constant) = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        constants.push(*constant);
    }
    // Every term must name a property of this variable, so no other column is involved.
    terms
        .iter()
        .all(|(entry, _)| column_of(entry).is_some())
        .then_some(constants)
}

/// The key a constant fixes for one key column, `None` when the value is NULL or not
/// representable (the mapping is then read exactly).
pub(super) fn constant_part(value: &Value, key: &GraphKeyColumn) -> Option<KeyPart> {
    key_part(value, &key.data_type)
}

/// One line per mapping for `EXPLAIN`.
pub(super) fn describe(access: &Access, plan: &Plan<'_>, graph: &PropertyGraphMeta) -> String {
    let mut parts = Vec::new();
    let start = |mapping: usize| {
        plan.positions[0].selection[mapping]
            .as_ref()
            .map(|columns| start_anchor(plan, &graph.vertices[mapping].key, columns).is_some())
    };
    for (mapping, meta) in graph.vertices.iter().enumerate() {
        if !access.vertex_read[mapping] {
            continue;
        }
        let path = if access.forced_exact {
            format!("exact(reason={FORCED_EXACT})")
        } else if let Some(key) = &access.vertex[mapping] {
            format!("point_lookup(index={})", key.index.name)
        } else {
            format!("exact(reason={NO_KEY_INDEX})")
        };
        let start = match start(mapping) {
            Some(true) if !access.forced_exact => "anchored",
            Some(_) => "scan",
            None => "unused",
        };
        parts.push(format!("vertex[{}]={path},start={start}", meta.alias));
    }
    for (mapping, meta) in graph.edges.iter().enumerate() {
        let Some(edge) = &access.edge[mapping] else {
            continue;
        };
        let path = if let Some(reason) = edge.reason {
            format!("exact(reason={reason})")
        } else {
            let names = [
                ("out", edge.source.as_ref()),
                ("in", edge.destination.as_ref()),
            ]
            .into_iter()
            .filter_map(|(side, key)| key.map(|key| format!("{side}:{}", key.index.name)))
            .collect::<Vec<_>>()
            .join("|");
            format!("indexed(index={names})")
        };
        parts.push(format!("edge[{}]={path}", meta.alias));
    }
    parts.join(" ")
}
