//! Exhaustive owned-field accounting for retained catalog metadata.

use super::heap::HeapBytes;

unit_enum_heap!(crate::embeddings::types::DistanceMetric { Cosine, L2, Dot });

metadata_heap!(crate::embeddings::types::HnswGraphNode {
    id,
    vector,
    magnitude,
    layers,
});

metadata_heap!(crate::embeddings::types::HnswGraphState {
    version,
    source_fingerprint,
    row_count,
    dimensions,
    metric,
    entry_point,
    max_layer,
    nodes,
});

metadata_heap!(crate::embeddings::types::HnswIndexOptions {
    version,
    m,
    ef_construction,
    ef_search,
});

metadata_heap!(crate::embeddings::types::IvfFlatIndexOptions {
    version,
    lists,
    probes,
    training_sample_size,
    training_seed,
});

metadata_heap!(crate::embeddings::types::IvfFlatTrainingState {
    version,
    source_fingerprint,
    trained,
    row_count,
    lists,
    probes,
    training_seed,
    centroid_ids,
    centroids,
    assignments,
    list_sizes,
});

metadata_heap!(crate::embeddings::types::VectorIndexMetadata {
    provider,
    model,
    dimensions,
    metric,
    index_type,
    hnsw,
    hnsw_graph,
    ivfflat,
    ivfflat_training,
});

metadata_heap!(crate::embeddings::types::VectorIndexRecord {
    collection,
    field,
    source_field,
    metadata,
});

unit_enum_heap!(crate::embeddings::types::VectorIndexType {
    BruteForce,
    Hnsw,
    IvfFlat
});
