use cntryl_lexkey::LexKey;

use super::{
    data_scoped_key, data_scoped_prefix, scoped_key, scoped_prefix, FAMILY_FULLTEXT_INDEX,
    FULLTEXT_ARTIFACT_DOCUMENT, FULLTEXT_ARTIFACT_MANIFEST, FULLTEXT_ARTIFACT_META,
    FULLTEXT_ARTIFACT_POSTINGS,
};

pub(crate) fn fulltext_index_key(collection: &str, name: &str) -> Vec<u8> {
    scoped_key(
        FAMILY_FULLTEXT_INDEX,
        collection,
        &[name.as_bytes(), &FULLTEXT_ARTIFACT_META],
    )
}

pub(crate) fn fulltext_index_collection_prefix(collection: &str) -> Vec<u8> {
    scoped_prefix(FAMILY_FULLTEXT_INDEX, collection, &[])
}

pub(crate) fn fulltext_index_manifest_key(
    collection: &str,
    index_name: &str,
    generation: u64,
) -> Vec<u8> {
    let encoded_generation = LexKey::encode_u64(generation);
    data_scoped_key(
        FAMILY_FULLTEXT_INDEX,
        collection,
        &[
            index_name.as_bytes(),
            &FULLTEXT_ARTIFACT_MANIFEST,
            encoded_generation.as_bytes(),
        ],
    )
}

pub(crate) fn fulltext_index_artifact_prefix(collection: &str, index_name: &str) -> Vec<u8> {
    data_scoped_prefix(FAMILY_FULLTEXT_INDEX, collection, &[index_name.as_bytes()])
}

pub(crate) fn fulltext_term_postings_key(
    collection: &str,
    index_name: &str,
    term: &str,
) -> Vec<u8> {
    data_scoped_key(
        FAMILY_FULLTEXT_INDEX,
        collection,
        &[
            index_name.as_bytes(),
            &FULLTEXT_ARTIFACT_POSTINGS,
            term.as_bytes(),
        ],
    )
}

pub(crate) fn fulltext_postings_prefix(collection: &str, index_name: &str) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_FULLTEXT_INDEX,
        collection,
        &[index_name.as_bytes(), &FULLTEXT_ARTIFACT_POSTINGS],
    )
}

pub(crate) fn fulltext_document_stats_key(
    collection: &str,
    index_name: &str,
    doc_id: &str,
) -> Vec<u8> {
    data_scoped_key(
        FAMILY_FULLTEXT_INDEX,
        collection,
        &[
            index_name.as_bytes(),
            &FULLTEXT_ARTIFACT_DOCUMENT,
            doc_id.as_bytes(),
        ],
    )
}

pub(crate) fn fulltext_document_stats_prefix(collection: &str, index_name: &str) -> Vec<u8> {
    data_scoped_prefix(
        FAMILY_FULLTEXT_INDEX,
        collection,
        &[index_name.as_bytes(), &FULLTEXT_ARTIFACT_DOCUMENT],
    )
}
