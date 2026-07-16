use cntryl_lexkey::LexKey;

use super::{
    encoded_u64_component, key, prefix, FAMILY_FULLTEXT_INDEX, FULLTEXT_ARTIFACT_DOCUMENT,
    FULLTEXT_ARTIFACT_MANIFEST, FULLTEXT_ARTIFACT_META, FULLTEXT_ARTIFACT_POSTINGS,
};

pub(crate) fn fulltext_index_key(relation_id: u64, index_id: u64) -> Vec<u8> {
    artifact_key(relation_id, index_id, &FULLTEXT_ARTIFACT_META, &[])
}

pub(crate) fn fulltext_index_collection_prefix(relation_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    prefix(FAMILY_FULLTEXT_INDEX, &[relation.as_slice()])
}

pub(crate) fn fulltext_index_manifest_key(
    relation_id: u64,
    index_id: u64,
    generation: u64,
) -> Vec<u8> {
    let encoded_generation = LexKey::encode_u64(generation);
    artifact_key(
        relation_id,
        index_id,
        &FULLTEXT_ARTIFACT_MANIFEST,
        &[encoded_generation.as_bytes()],
    )
}

pub(crate) fn fulltext_index_artifact_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    let index = encoded_u64_component(index_id);
    prefix(
        FAMILY_FULLTEXT_INDEX,
        &[relation.as_slice(), index.as_slice()],
    )
}

pub(crate) fn fulltext_term_postings_prefix(
    relation_id: u64,
    index_id: u64,
    term: &str,
) -> Vec<u8> {
    let mut value = artifact_key(
        relation_id,
        index_id,
        &FULLTEXT_ARTIFACT_POSTINGS,
        &[term.as_bytes()],
    );
    value.push(LexKey::SEPARATOR);
    value
}

pub(crate) fn fulltext_term_postings_block_key(
    relation_id: u64,
    index_id: u64,
    term: &str,
    block: usize,
) -> Vec<u8> {
    let block = LexKey::encode_u64(u64::try_from(block).unwrap_or(u64::MAX));
    artifact_key(
        relation_id,
        index_id,
        &FULLTEXT_ARTIFACT_POSTINGS,
        &[term.as_bytes(), block.as_bytes()],
    )
}

pub(crate) fn fulltext_postings_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
    let mut value = artifact_key(relation_id, index_id, &FULLTEXT_ARTIFACT_POSTINGS, &[]);
    value.push(LexKey::SEPARATOR);
    value
}

pub(crate) fn fulltext_document_stats_key(
    relation_id: u64,
    index_id: u64,
    doc_id: &str,
) -> Vec<u8> {
    artifact_key(
        relation_id,
        index_id,
        &FULLTEXT_ARTIFACT_DOCUMENT,
        &[doc_id.as_bytes()],
    )
}

pub(crate) fn fulltext_document_stats_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
    let mut value = artifact_key(relation_id, index_id, &FULLTEXT_ARTIFACT_DOCUMENT, &[]);
    value.push(LexKey::SEPARATOR);
    value
}

fn artifact_key(relation_id: u64, index_id: u64, artifact: &[u8], suffix: &[&[u8]]) -> Vec<u8> {
    let relation = encoded_u64_component(relation_id);
    let index = encoded_u64_component(index_id);
    let mut components = vec![relation.as_slice(), index.as_slice(), artifact];
    components.extend_from_slice(suffix);
    key(FAMILY_FULLTEXT_INDEX, &components)
}
