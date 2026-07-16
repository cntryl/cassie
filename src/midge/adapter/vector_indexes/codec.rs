use crate::app::CassieError;
use crate::embeddings::{DistanceMetric, NormalizedVectorRecord};

const FORMAT_VERSION: u8 = 1;
const HNSW_FORMAT: u8 = 2;
const STATE_FORMAT: u8 = 3;

#[derive(Debug)]
pub(super) struct PersistedHnswManifest {
    pub(super) version: u32,
    pub(super) source_fingerprint: u64,
    pub(super) row_count: usize,
    pub(super) dimensions: usize,
    pub(super) metric: DistanceMetric,
    pub(super) entry_point: Option<String>,
    pub(super) max_layer: usize,
}

#[derive(Debug)]
pub(super) struct PersistedIvfManifest {
    pub(super) version: u32,
    pub(super) source_fingerprint: u64,
    pub(super) trained: bool,
    pub(super) row_count: usize,
    pub(super) lists: usize,
    pub(super) probes: usize,
    pub(super) training_seed: u64,
    pub(super) centroid_ids: Vec<String>,
    pub(super) centroids: Vec<Vec<f32>>,
    pub(super) list_sizes: Vec<usize>,
    pub(super) membership_count: usize,
}

#[derive(Debug)]
pub(super) struct PersistedVectorIndexState {
    pub(super) built_generation: u64,
    pub(super) hnsw_graph: Option<PersistedHnswManifest>,
    pub(super) ivfflat_training: Option<PersistedIvfManifest>,
}

pub(super) fn encode_vector_index_state(
    state: &PersistedVectorIndexState,
) -> Result<Vec<u8>, CassieError> {
    let mut out = vec![STATE_FORMAT];
    out.extend_from_slice(&state.built_generation.to_be_bytes());
    out.push(u8::from(state.hnsw_graph.is_some()));
    out.push(u8::from(state.ivfflat_training.is_some()));
    if let Some(graph) = &state.hnsw_graph {
        put_u32(graph.version, &mut out);
        put_u64(graph.source_fingerprint, &mut out);
        put_usize(graph.row_count, &mut out)?;
        put_usize(graph.dimensions, &mut out)?;
        out.push(metric_tag(graph.metric));
        put_optional_string(graph.entry_point.as_deref(), &mut out)?;
        put_usize(graph.max_layer, &mut out)?;
    }
    if let Some(training) = &state.ivfflat_training {
        put_u32(training.version, &mut out);
        put_u64(training.source_fingerprint, &mut out);
        out.push(u8::from(training.trained));
        put_usize(training.row_count, &mut out)?;
        put_usize(training.lists, &mut out)?;
        put_usize(training.probes, &mut out)?;
        put_u64(training.training_seed, &mut out);
        put_strings(&training.centroid_ids, &mut out)?;
        put_vectors(&training.centroids, &mut out)?;
        put_usizes(&training.list_sizes, &mut out)?;
        put_usize(training.membership_count, &mut out)?;
    }
    Ok(out)
}

pub(super) fn decode_vector_index_state(
    bytes: &[u8],
) -> Result<PersistedVectorIndexState, CassieError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.byte()? != STATE_FORMAT {
        return Err(CassieError::Parse(
            "invalid vector index state format".to_string(),
        ));
    }
    let built_generation = cursor.u64()?;
    let has_hnsw = cursor.boolean()?;
    let has_ivf = cursor.boolean()?;
    let hnsw_graph = has_hnsw
        .then(|| decode_hnsw_manifest(&mut cursor))
        .transpose()?;
    let ivfflat_training = has_ivf
        .then(|| decode_ivf_manifest(&mut cursor))
        .transpose()?;
    cursor.finish()?;
    Ok(PersistedVectorIndexState {
        built_generation,
        hnsw_graph,
        ivfflat_training,
    })
}

fn decode_hnsw_manifest(cursor: &mut Cursor<'_>) -> Result<PersistedHnswManifest, CassieError> {
    Ok(PersistedHnswManifest {
        version: cursor.u32()?,
        source_fingerprint: cursor.u64()?,
        row_count: cursor.usize()?,
        dimensions: cursor.usize()?,
        metric: decode_metric(cursor.byte()?)?,
        entry_point: cursor.optional_string()?,
        max_layer: cursor.usize()?,
    })
}

fn decode_ivf_manifest(cursor: &mut Cursor<'_>) -> Result<PersistedIvfManifest, CassieError> {
    Ok(PersistedIvfManifest {
        version: cursor.u32()?,
        source_fingerprint: cursor.u64()?,
        trained: cursor.boolean()?,
        row_count: cursor.usize()?,
        lists: cursor.usize()?,
        probes: cursor.usize()?,
        training_seed: cursor.u64()?,
        centroid_ids: cursor.strings()?,
        centroids: cursor.vectors()?,
        list_sizes: cursor.usizes()?,
        membership_count: cursor.usize()?,
    })
}

pub(crate) fn encode_normalized_vector(
    record: &NormalizedVectorRecord,
) -> Result<Vec<u8>, CassieError> {
    if record.values.len() != record.dimensions {
        return Err(CassieError::Parse(
            "normalized vector dimension mismatch".to_string(),
        ));
    }
    let dimensions = u32::try_from(record.dimensions)
        .map_err(|_| CassieError::Parse("normalized vector is too large".to_string()))?;
    let mut out = Vec::with_capacity(31 + record.values.len().saturating_mul(4));
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&record.normalization_version.to_be_bytes());
    out.extend_from_slice(&record.built_generation.to_be_bytes());
    out.extend_from_slice(&dimensions.to_be_bytes());
    out.push(match record.metric {
        DistanceMetric::Cosine => 0,
        DistanceMetric::L2 => 1,
        DistanceMetric::Dot => 2,
    });
    out.push(u8::from(record.payload_available));
    out.extend_from_slice(&record.magnitude.to_bits().to_be_bytes());
    for value in &record.values {
        out.extend_from_slice(&value.to_bits().to_be_bytes());
    }
    Ok(out)
}

pub(crate) fn decode_normalized_vector(
    bytes: &[u8],
    collection: &str,
    field: &str,
    id: &str,
) -> Result<NormalizedVectorRecord, CassieError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.byte()? != FORMAT_VERSION {
        return Err(CassieError::Parse(
            "invalid normalized vector format".to_string(),
        ));
    }
    let normalization_version = cursor.u32()?;
    let built_generation = cursor.u64()?;
    let dimensions = usize::try_from(cursor.u32()?)
        .map_err(|_| CassieError::Parse("normalized vector dimension overflow".to_string()))?;
    let metric = match cursor.byte()? {
        0 => DistanceMetric::Cosine,
        1 => DistanceMetric::L2,
        2 => DistanceMetric::Dot,
        _ => {
            return Err(CassieError::Parse(
                "invalid normalized vector metric".to_string(),
            ))
        }
    };
    let payload_available = match cursor.byte()? {
        0 => false,
        1 => true,
        _ => {
            return Err(CassieError::Parse(
                "invalid normalized vector flags".to_string(),
            ))
        }
    };
    let magnitude = f64::from_bits(cursor.u64()?);
    let values = (0..dimensions)
        .map(|_| cursor.u32().map(f32::from_bits))
        .collect::<Result<Vec<_>, _>>()?;
    cursor.finish()?;
    Ok(NormalizedVectorRecord {
        collection: collection.to_string(),
        field: field.to_string(),
        id: id.to_string(),
        built_generation,
        dimensions,
        metric,
        normalization_version,
        payload_available,
        magnitude,
        values,
    })
}

pub(super) fn encode_hnsw_node(
    node: &crate::embeddings::HnswGraphNode,
) -> Result<Vec<u8>, CassieError> {
    let vector_len = u32::try_from(node.vector.len())
        .map_err(|_| CassieError::Parse("HNSW vector is too large".to_string()))?;
    let layer_count = u32::try_from(node.layers.len())
        .map_err(|_| CassieError::Parse("HNSW node has too many layers".to_string()))?;
    let mut out = Vec::new();
    out.push(HNSW_FORMAT);
    out.extend_from_slice(&vector_len.to_be_bytes());
    for value in &node.vector {
        out.extend_from_slice(&value.to_bits().to_be_bytes());
    }
    out.extend_from_slice(&node.magnitude.to_bits().to_be_bytes());
    out.extend_from_slice(&layer_count.to_be_bytes());
    for layer in &node.layers {
        let neighbor_count = u32::try_from(layer.len())
            .map_err(|_| CassieError::Parse("HNSW layer is too large".to_string()))?;
        out.extend_from_slice(&neighbor_count.to_be_bytes());
        for neighbor in layer {
            let len = u32::try_from(neighbor.len())
                .map_err(|_| CassieError::Parse("HNSW neighbor id is too long".to_string()))?;
            out.extend_from_slice(&len.to_be_bytes());
            out.extend_from_slice(neighbor.as_bytes());
        }
    }
    Ok(out)
}

pub(super) fn decode_hnsw_node(
    bytes: &[u8],
    id: &str,
) -> Result<crate::embeddings::HnswGraphNode, CassieError> {
    let mut cursor = Cursor::new(bytes);
    if cursor.byte()? != HNSW_FORMAT {
        return Err(CassieError::Parse("invalid HNSW node format".to_string()));
    }
    let vector_len = usize::try_from(cursor.u32()?)
        .map_err(|_| CassieError::Parse("HNSW vector length overflow".to_string()))?;
    let vector = (0..vector_len)
        .map(|_| cursor.u32().map(f32::from_bits))
        .collect::<Result<Vec<_>, _>>()?;
    let magnitude = f64::from_bits(cursor.u64()?);
    let layer_count = usize::try_from(cursor.u32()?)
        .map_err(|_| CassieError::Parse("HNSW layer count overflow".to_string()))?;
    let mut layers = Vec::with_capacity(layer_count);
    for _ in 0..layer_count {
        let count = usize::try_from(cursor.u32()?)
            .map_err(|_| CassieError::Parse("HNSW neighbor count overflow".to_string()))?;
        let mut layer = Vec::with_capacity(count);
        for _ in 0..count {
            let len = usize::try_from(cursor.u32()?)
                .map_err(|_| CassieError::Parse("HNSW neighbor length overflow".to_string()))?;
            let neighbor = std::str::from_utf8(cursor.take(len)?)
                .map_err(|error| CassieError::Parse(format!("invalid HNSW neighbor: {error}")))?;
            layer.push(neighbor.to_string());
        }
        layers.push(layer);
    }
    cursor.finish()?;
    Ok(crate::embeddings::HnswGraphNode {
        id: id.to_string(),
        vector,
        magnitude,
        layers,
    })
}

fn metric_tag(metric: DistanceMetric) -> u8 {
    match metric {
        DistanceMetric::Cosine => 0,
        DistanceMetric::L2 => 1,
        DistanceMetric::Dot => 2,
    }
}

fn decode_metric(tag: u8) -> Result<DistanceMetric, CassieError> {
    match tag {
        0 => Ok(DistanceMetric::Cosine),
        1 => Ok(DistanceMetric::L2),
        2 => Ok(DistanceMetric::Dot),
        _ => Err(CassieError::Parse(
            "invalid vector index metric".to_string(),
        )),
    }
}

fn put_u32(value: u32, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(value: u64, out: &mut Vec<u8>) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(value: usize, out: &mut Vec<u8>) -> Result<(), CassieError> {
    let value = u64::try_from(value)
        .map_err(|_| CassieError::Parse("vector index count overflow".to_string()))?;
    put_u64(value, out);
    Ok(())
}

fn put_string(value: &str, out: &mut Vec<u8>) -> Result<(), CassieError> {
    let len = u32::try_from(value.len())
        .map_err(|_| CassieError::Parse("vector index string is too long".to_string()))?;
    put_u32(len, out);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn put_optional_string(value: Option<&str>, out: &mut Vec<u8>) -> Result<(), CassieError> {
    out.push(u8::from(value.is_some()));
    if let Some(value) = value {
        put_string(value, out)?;
    }
    Ok(())
}

fn put_strings(values: &[String], out: &mut Vec<u8>) -> Result<(), CassieError> {
    put_usize(values.len(), out)?;
    for value in values {
        put_string(value, out)?;
    }
    Ok(())
}

fn put_vectors(values: &[Vec<f32>], out: &mut Vec<u8>) -> Result<(), CassieError> {
    put_usize(values.len(), out)?;
    for vector in values {
        put_usize(vector.len(), out)?;
        for value in vector {
            put_u32(value.to_bits(), out);
        }
    }
    Ok(())
}

fn put_usizes(values: &[usize], out: &mut Vec<u8>) -> Result<(), CassieError> {
    put_usize(values.len(), out)?;
    for value in values {
        put_usize(*value, out)?;
    }
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn byte(&mut self) -> Result<u8, CassieError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, CassieError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, CassieError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn usize(&mut self) -> Result<usize, CassieError> {
        usize::try_from(self.u64()?)
            .map_err(|_| CassieError::Parse("vector index count overflow".to_string()))
    }

    fn boolean(&mut self) -> Result<bool, CassieError> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CassieError::Parse(
                "invalid vector index boolean".to_string(),
            )),
        }
    }

    fn string(&mut self) -> Result<String, CassieError> {
        let len = usize::try_from(self.u32()?)
            .map_err(|_| CassieError::Parse("vector index string overflow".to_string()))?;
        std::str::from_utf8(self.take(len)?)
            .map(str::to_string)
            .map_err(|error| CassieError::Parse(format!("invalid vector index string: {error}")))
    }

    fn optional_string(&mut self) -> Result<Option<String>, CassieError> {
        self.boolean()?.then(|| self.string()).transpose()
    }

    fn strings(&mut self) -> Result<Vec<String>, CassieError> {
        (0..self.usize()?).map(|_| self.string()).collect()
    }

    fn vectors(&mut self) -> Result<Vec<Vec<f32>>, CassieError> {
        (0..self.usize()?)
            .map(|_| {
                (0..self.usize()?)
                    .map(|_| self.u32().map(f32::from_bits))
                    .collect()
            })
            .collect()
    }

    fn usizes(&mut self) -> Result<Vec<usize>, CassieError> {
        (0..self.usize()?).map(|_| self.usize()).collect()
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], CassieError> {
        self.take(N)?
            .try_into()
            .map_err(|_| CassieError::Parse("truncated normalized vector".to_string()))
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], CassieError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| CassieError::Parse("normalized vector offset overflow".to_string()))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| CassieError::Parse("truncated normalized vector".to_string()))?;
        self.offset = end;
        Ok(value)
    }

    fn finish(self) -> Result<(), CassieError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(CassieError::Parse(
                "trailing normalized vector bytes".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_roundtrip_compact_normalized_vector_without_identity_fields() {
        // Arrange
        let record = NormalizedVectorRecord {
            collection: "public.items".to_string(),
            field: "embedding".to_string(),
            id: "row-1".to_string(),
            built_generation: 7,
            dimensions: 3,
            metric: DistanceMetric::Cosine,
            normalization_version: 1,
            payload_available: true,
            magnitude: 2.5,
            values: vec![0.1, 0.2, 0.3],
        };

        // Act
        let encoded = encode_normalized_vector(&record).expect("encode vector");
        let decoded =
            decode_normalized_vector(&encoded, &record.collection, &record.field, &record.id)
                .expect("decode vector");

        // Assert
        assert_eq!(encoded[0], FORMAT_VERSION);
        assert!(!encoded
            .windows(record.collection.len())
            .any(|window| window == record.collection.as_bytes()));
        assert_eq!(decoded, record);
    }

    #[test]
    fn should_roundtrip_binary_hnsw_node_without_duplicate_id() {
        // Arrange
        let node = crate::embeddings::HnswGraphNode {
            id: "node-1".to_string(),
            vector: vec![0.25, 0.75],
            magnitude: 1.0,
            layers: vec![vec!["node-2".to_string(), "node-3".to_string()]],
        };

        // Act
        let encoded = encode_hnsw_node(&node).expect("encode HNSW node");
        let decoded = decode_hnsw_node(&encoded, &node.id).expect("decode HNSW node");

        // Assert
        assert_eq!(decoded, node);
        assert!(!encoded
            .windows(node.id.len())
            .any(|window| window == node.id.as_bytes()));
    }
}
