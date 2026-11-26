//! Tier 1 kernels of the mapped graph path values: construction and validation, ordering and
//! key-part normalisation. No storage, catalog lookup or SQL is involved.

use std::cmp::Ordering;

use cassie_core::catalog::graph_schema::build_graph_types;
use cassie_engine::types::graph::{
    compare_paths, validate_record, Direction, ElementKind, ElementRef, GraphDescriptors, KeyPart,
    Path,
};
use cassie_engine::types::typed::TypedValue;
use cassie_engine::types::Value;
use uuid::Uuid;

const PATH_COUNT: usize = 16;

pub struct GraphPathKernel {
    descriptors: GraphDescriptors,
    paths: Vec<Path>,
    key_tuple: Vec<KeyPart>,
}

fn id(last: u8) -> Uuid {
    Uuid::from_bytes([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, last])
}

fn element(kind: ElementKind, mapping: u8, key: Vec<KeyPart>) -> ElementRef {
    ElementRef {
        database_id: id(1),
        graph_id: id(2),
        mapping_id: id(mapping),
        kind,
        key,
    }
}

fn vertex(index: i64) -> ElementRef {
    element(
        ElementKind::Vertex,
        3,
        vec![KeyPart::Text("tenant-a".to_owned()), KeyPart::BigInt(index)],
    )
}

fn edge(index: i64) -> ElementRef {
    element(ElementKind::Edge, 4, vec![KeyPart::BigInt(index)])
}

/// Paths of three edges that share every identity except the last edge key, so ordering
/// comparisons walk the whole common prefix before they decide.
fn sample_paths() -> Vec<Path> {
    (0..PATH_COUNT)
        .map(|variant| {
            let variant = i64::try_from(variant).expect("small variant");
            Path {
                vertices: (0..=3).map(vertex).collect(),
                edges: vec![edge(1), edge(2), edge(100 + variant)],
                directions: vec![Direction::Forward, Direction::Reverse, Direction::Forward],
            }
        })
        .collect()
}

impl GraphPathKernel {
    #[must_use]
    pub fn new() -> Self {
        let mut next = 100;
        let types = build_graph_types("main", || {
            next += 1;
            Ok::<_, std::convert::Infallible>(next)
        })
        .expect("build the reserved graph types");
        let descriptors = GraphDescriptors {
            key_part: types[0].descriptor(),
            element_ref: types[1].descriptor(),
            path: types[2].descriptor(),
        };
        Self {
            descriptors,
            paths: sample_paths(),
            key_tuple: vec![
                KeyPart::SmallInt(-5),
                KeyPart::Int(7),
                KeyPart::BigInt(9_007_199_254_740_993),
                KeyPart::Text("a\"b".to_owned()),
                KeyPart::Uuid(id(9)),
            ],
        }
    }

    /// Builds the `graph_path` record of every sample path and validates it.
    pub fn construct_and_validate(&self) -> u64 {
        let mut validated = 0_u64;
        for path in &self.paths {
            let value = path
                .to_value(&self.descriptors)
                .expect("sample path builds");
            let Value::Typed(TypedValue::Record(record)) = &value else {
                panic!("a path value is a record");
            };
            validate_record(record).expect("sample path validates");
            std::hint::black_box(&value);
            validated += 1;
        }
        validated
    }

    /// Compares each sample path with its neighbour in the total order.
    pub fn compare_neighbours(&self) -> u64 {
        let mut less = 0_u64;
        let mut comparisons = 0_u64;
        for pair in self.paths.windows(2) {
            if compare_paths(&pair[0], &pair[1]) == Ordering::Less {
                less += 1;
            }
            comparisons += 1;
        }
        assert_eq!(less, comparisons, "sample paths are strictly increasing");
        comparisons
    }

    /// Encodes each key part as a record and decodes it back to the typed part.
    pub fn normalize_key_tuple(&self) -> u64 {
        let mut normalized = 0_u64;
        for part in &self.key_tuple {
            let value = part.to_value(&self.descriptors).expect("key part builds");
            let Value::Typed(TypedValue::Record(record)) = &value else {
                panic!("a key part value is a record");
            };
            let decoded = KeyPart::from_record(record).expect("key part decodes");
            assert_eq!(&decoded, part, "a key part survives normalisation");
            std::hint::black_box(decoded.value_text());
            normalized += 1;
        }
        normalized
    }

    pub fn key_tuple_len(&self) -> u64 {
        u64::try_from(self.key_tuple.len()).expect("small tuple")
    }

    pub fn comparisons_per_batch(&self) -> u64 {
        u64::try_from(self.paths.len() - 1).expect("small path set")
    }

    pub fn paths_per_batch(&self) -> u64 {
        u64::try_from(self.paths.len()).expect("small path set")
    }
}

impl Default for GraphPathKernel {
    fn default() -> Self {
        Self::new()
    }
}
