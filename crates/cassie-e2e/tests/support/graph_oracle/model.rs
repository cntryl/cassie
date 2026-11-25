//! In-memory graph model, element identity and the contract's path order.

use std::collections::{BTreeMap, BTreeSet};

/// One typed key component. Variant order is the contract's type order
/// (smallint, integer, bigint, text, uuid); derived `Ord` compares the type
/// first and then the value (text in C byte order, UUID as unsigned 16 bytes).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyPart {
    SmallInt(i16),
    Integer(i32),
    BigInt(i64),
    Text(String),
    Uuid([u8; 16]),
}

impl KeyPart {
    /// A UUID key part from its numeric value.
    #[must_use]
    pub fn uuid(value: u128) -> Self {
        Self::Uuid(value.to_be_bytes())
    }

    /// A text key part.
    #[must_use]
    pub fn text(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

/// Element kind; vertices order before edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Vertex,
    Edge,
}

/// Durable element reference. Field order is the contract's comparison order:
/// database ID, graph ID, kind, mapping ID, then the key tuple.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementRef {
    pub database_id: u128,
    pub graph_id: u128,
    pub kind: Kind,
    pub mapping_id: u128,
    pub key: Vec<KeyPart>,
}

/// A vertex reference in the oracle's single database and graph.
#[must_use]
pub fn vertex_ref(mapping_id: u128, key: Vec<KeyPart>) -> ElementRef {
    ElementRef {
        database_id: 1,
        graph_id: 1,
        kind: Kind::Vertex,
        mapping_id,
        key,
    }
}

/// An edge reference in the oracle's single database and graph.
#[must_use]
pub fn edge_ref(mapping_id: u128, key: Vec<KeyPart>) -> ElementRef {
    ElementRef {
        kind: Kind::Edge,
        ..vertex_ref(mapping_id, key)
    }
}

/// Small scalar property values. `Num` holds exact micro-units
/// (`value * 1_000_000`); `NonFinite` models NaN and infinities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Scalar {
    Null,
    Bool(bool),
    Int(i64),
    Text(String),
    Num(i128),
    NonFinite,
}

/// Fixed scale of `Scalar::Num` and weighted costs.
pub const SCALE: i128 = 1_000_000;

#[derive(Clone, Debug)]
pub struct Vertex {
    pub alias: String,
    pub id: ElementRef,
    pub labels: BTreeSet<String>,
    pub props: BTreeMap<String, Scalar>,
}

impl Vertex {
    #[must_use]
    pub fn new(alias: &str, mapping_id: u128, key: Vec<KeyPart>, labels: &[&str]) -> Self {
        Self {
            alias: alias.to_string(),
            id: vertex_ref(mapping_id, key),
            labels: labels.iter().map(|l| (*l).to_string()).collect(),
            props: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn prop(mut self, name: &str, value: Scalar) -> Self {
        self.props.insert(name.to_string(), value);
        self
    }
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub alias: String,
    pub id: ElementRef,
    pub labels: BTreeSet<String>,
    pub props: BTreeMap<String, Scalar>,
    pub source: ElementRef,
    pub destination: ElementRef,
}

impl Edge {
    #[must_use]
    pub fn new(
        alias: &str,
        mapping_id: u128,
        key: Vec<KeyPart>,
        labels: &[&str],
        source: &ElementRef,
        destination: &ElementRef,
    ) -> Self {
        Self {
            alias: alias.to_string(),
            id: edge_ref(mapping_id, key),
            labels: labels.iter().map(|l| (*l).to_string()).collect(),
            props: BTreeMap::new(),
            source: source.clone(),
            destination: destination.clone(),
        }
    }

    #[must_use]
    pub fn prop(mut self, name: &str, value: Scalar) -> Self {
        self.props.insert(name.to_string(), value);
        self
    }
}

/// Edge traversal direction of one pattern step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// `-[e]->`: source to destination, recorded as direction 1.
    Forward,
    /// `<-[e]-`: destination to source, recorded as direction 2.
    Backward,
    /// `-[e]-`: either way; a self-loop is one step recorded as direction 1.
    Undirected,
}

/// One traversal choice: the edge, the recorded direction value and the
/// vertex reached.
pub struct Hop<'a> {
    pub edge: &'a Edge,
    pub direction: u8,
    pub to: &'a Vertex,
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
}

impl Graph {
    #[must_use]
    pub fn vertex(&self, id: &ElementRef) -> Option<&Vertex> {
        self.vertices.iter().find(|v| &v.id == id)
    }

    /// Edges whose source and destination vertices both exist. An edge with a
    /// missing endpoint is not graph-visible ("Edges whose endpoint vertex is
    /// absent" are excluded).
    pub fn live_edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges
            .iter()
            .filter(|e| self.vertex(&e.source).is_some() && self.vertex(&e.destination).is_some())
    }

    /// Every traversal choice leaving `from` in `direction` over live edges.
    #[must_use]
    pub fn hops(&self, from: &ElementRef, direction: Direction) -> Vec<Hop<'_>> {
        let mut out = Vec::new();
        for edge in self.live_edges() {
            let forward = direction != Direction::Backward && &edge.source == from;
            let backward = direction != Direction::Forward && &edge.destination == from;
            let self_loop = edge.source == edge.destination;
            if self_loop && direction == Direction::Undirected {
                // One step with the canonical stored direction, never two.
                if forward {
                    out.push(self.hop(edge, 1, &edge.destination));
                }
                continue;
            }
            if forward {
                out.push(self.hop(edge, 1, &edge.destination));
            }
            if backward {
                out.push(self.hop(edge, 2, &edge.source));
            }
        }
        out
    }

    fn hop<'a>(&'a self, edge: &'a Edge, direction: u8, to: &ElementRef) -> Hop<'a> {
        let to = self.vertex(to).expect("live edge endpoint exists");
        Hop {
            edge,
            direction,
            to,
        }
    }
}

/// A matched path: interleaved vertices, edges and direction values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    pub vertices: Vec<ElementRef>,
    pub edges: Vec<ElementRef>,
    pub directions: Vec<u8>,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Item<'a> {
    Vertex(&'a ElementRef),
    Edge(&'a ElementRef),
    Direction(u8),
}

impl Path {
    /// A zero-hop path at `vertex`.
    #[must_use]
    pub fn single(vertex: &ElementRef) -> Self {
        Self {
            vertices: vec![vertex.clone()],
            edges: Vec::new(),
            directions: Vec::new(),
        }
    }

    #[must_use]
    pub fn hop_count(&self) -> usize {
        self.edges.len()
    }

    #[must_use]
    pub fn start(&self) -> &ElementRef {
        &self.vertices[0]
    }

    #[must_use]
    pub fn end(&self) -> &ElementRef {
        &self.vertices[self.vertices.len() - 1]
    }

    /// A copy extended by one hop.
    #[must_use]
    pub fn extended(&self, hop: &Hop<'_>) -> Self {
        let mut next = self.clone();
        next.edges.push(hop.edge.id.clone());
        next.directions.push(hop.direction);
        next.vertices.push(hop.to.id.clone());
        next
    }

    /// The contract's sequence `V0, E0, direction0, V1, E1, direction1, V2, ...`.
    fn sequence(&self) -> Vec<Item<'_>> {
        let mut items = vec![Item::Vertex(&self.vertices[0])];
        for (i, edge) in self.edges.iter().enumerate() {
            items.push(Item::Edge(edge));
            items.push(Item::Direction(self.directions[i]));
            items.push(Item::Vertex(&self.vertices[i + 1]));
        }
        items
    }
}

/// Lexicographic over the interleaved sequence; forward (1) before reverse
/// (2); the shorter sequence first when one is a prefix of the other.
impl Ord for Path {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.sequence().cmp(&other.sequence())
    }
}

impl PartialOrd for Path {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
