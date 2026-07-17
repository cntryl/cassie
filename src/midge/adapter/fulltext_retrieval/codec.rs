use std::collections::BTreeMap;

use crate::app::CassieError;

use super::{
    FulltextIndexMetadata, FulltextManifest, PersistedFulltextDocumentStats,
    PersistedFulltextPosting,
};

const POSTINGS_MAGIC: &[u8; 4] = b"FTB1";
const DOCUMENT_MAGIC: &[u8; 4] = b"FTD1";
const METADATA_MAGIC: &[u8; 4] = b"FTM1";
const MANIFEST_MAGIC: &[u8; 4] = b"FTG1";
pub(super) const MAX_POSTING_BLOCK_BYTES: usize = 64 * 1024;

pub(super) fn encode_postings(
    postings: &[PersistedFulltextPosting],
) -> Result<Vec<u8>, CassieError> {
    let mut out = Vec::new();
    out.extend_from_slice(POSTINGS_MAGIC);
    write_varint(u64::try_from(postings.len()).unwrap_or(u64::MAX), &mut out);
    let mut previous = "";
    for posting in postings {
        let common = common_prefix_len(previous.as_bytes(), posting.document_id.as_bytes());
        let suffix = &posting.document_id.as_bytes()[common..];
        write_varint(u64::try_from(common).unwrap_or(u64::MAX), &mut out);
        write_bytes(suffix, &mut out);
        write_varint(
            u64::try_from(posting.term_frequency).unwrap_or(u64::MAX),
            &mut out,
        );
        previous = &posting.document_id;
    }
    if out.len() > MAX_POSTING_BLOCK_BYTES {
        return Err(CassieError::Execution(format!(
            "fulltext posting block exceeds {MAX_POSTING_BLOCK_BYTES} bytes"
        )));
    }
    Ok(out)
}

pub(super) fn encode_posting_blocks(
    postings: &[PersistedFulltextPosting],
) -> Result<Vec<Vec<u8>>, CassieError> {
    let mut blocks = Vec::new();
    let mut start = 0_usize;
    let mut index = 0_usize;
    let mut body_bytes = 0_usize;
    while start < postings.len() {
        let previous = index
            .checked_sub(1)
            .filter(|_| index > start)
            .map_or("", |previous| postings[previous].document_id.as_str());
        let entry_bytes = encoded_posting_entry_len(previous, &postings[index]);
        let count = index - start + 1;
        let encoded_bytes = 4 + varint_len(count) + body_bytes + entry_bytes;
        if encoded_bytes > MAX_POSTING_BLOCK_BYTES && index > start {
            blocks.push(encode_postings(&postings[start..index])?);
            start = index;
            body_bytes = 0;
            continue;
        }
        body_bytes += entry_bytes;
        index += 1;
        if index == postings.len() {
            blocks.push(encode_postings(&postings[start..index])?);
            break;
        }
    }
    if blocks.is_empty() {
        blocks.push(encode_postings(&[])?);
    }
    Ok(blocks)
}

fn encoded_posting_entry_len(previous: &str, posting: &PersistedFulltextPosting) -> usize {
    let common = common_prefix_len(previous.as_bytes(), posting.document_id.as_bytes());
    let suffix = posting.document_id.len() - common;
    varint_len(common) + varint_len(suffix) + suffix + varint_len(posting.term_frequency)
}

fn varint_len(value: usize) -> usize {
    let bits = usize::BITS - value.max(1).leading_zeros();
    usize::try_from(bits.div_ceil(7)).unwrap_or(usize::MAX)
}

pub(super) fn decode_postings(bytes: &[u8]) -> Result<Vec<PersistedFulltextPosting>, CassieError> {
    let mut cursor = Cursor::new(bytes);
    cursor.expect(POSTINGS_MAGIC)?;
    let count = cursor.read_usize()?;
    let mut postings = Vec::with_capacity(count);
    let mut previous = String::new();
    for _ in 0..count {
        let common = cursor.read_usize()?;
        if common > previous.len() || !previous.is_char_boundary(common) {
            return Err(CassieError::Parse(
                "invalid fulltext posting prefix length".to_string(),
            ));
        }
        let suffix = std::str::from_utf8(cursor.read_bytes()?)
            .map_err(|error| CassieError::Parse(format!("invalid posting id: {error}")))?;
        let mut document_id = previous[..common].to_string();
        document_id.push_str(suffix);
        let term_frequency = cursor.read_usize()?;
        postings.push(PersistedFulltextPosting {
            document_id: document_id.clone(),
            term_frequency,
        });
        previous = document_id;
    }
    cursor.finish()?;
    Ok(postings)
}

pub(super) fn encode_document_stats(stats: &PersistedFulltextDocumentStats) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(DOCUMENT_MAGIC);
    write_varint(
        u64::try_from(stats.doc_length).unwrap_or(u64::MAX),
        &mut out,
    );
    write_varint(
        u64::try_from(stats.term_counts.len()).unwrap_or(u64::MAX),
        &mut out,
    );
    for (term, count) in &stats.term_counts {
        write_bytes(term.as_bytes(), &mut out);
        write_varint(u64::try_from(*count).unwrap_or(u64::MAX), &mut out);
    }
    out
}

pub(super) fn decode_document_stats(
    bytes: &[u8],
) -> Result<PersistedFulltextDocumentStats, CassieError> {
    let mut cursor = Cursor::new(bytes);
    cursor.expect(DOCUMENT_MAGIC)?;
    let doc_length = cursor.read_usize()?;
    let count = cursor.read_usize()?;
    let mut term_counts = BTreeMap::new();
    for _ in 0..count {
        let term = std::str::from_utf8(cursor.read_bytes()?)
            .map_err(|error| CassieError::Parse(format!("invalid document term: {error}")))?
            .to_string();
        term_counts.insert(term, cursor.read_usize()?);
    }
    cursor.finish()?;
    Ok(PersistedFulltextDocumentStats {
        doc_length,
        term_counts,
    })
}

pub(super) fn encode_metadata(metadata: &FulltextIndexMetadata) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(METADATA_MAGIC);
    write_varint(u64::from(metadata.version), &mut out);
    write_varint(metadata.built_generation, &mut out);
    write_varint(
        u64::try_from(metadata.total_documents).unwrap_or(u64::MAX),
        &mut out,
    );
    write_varint(
        u64::try_from(metadata.documents_with_text).unwrap_or(u64::MAX),
        &mut out,
    );
    out.extend_from_slice(&metadata.average_document_length.to_bits().to_be_bytes());
    for value in [
        &metadata.analyzer.name,
        &metadata.analyzer.tokenizer,
        &metadata.analyzer.stop_words,
        &metadata.analyzer.stemming,
    ] {
        write_bytes(value.as_bytes(), &mut out);
    }
    out.push(u8::from(metadata.analyzer.case_folding));
    out.push(u8::from(metadata.analyzer.accent_folding));
    out
}

pub(super) fn decode_metadata(bytes: &[u8]) -> Result<FulltextIndexMetadata, CassieError> {
    let mut cursor = Cursor::new(bytes);
    cursor.expect(METADATA_MAGIC)?;
    let version = cursor.read_u32()?;
    let built_generation = cursor.read_varint()?;
    let total_documents = cursor.read_usize()?;
    let documents_with_text = cursor.read_usize()?;
    let average_document_length = cursor.read_f64()?;
    let name = cursor.read_string()?;
    let tokenizer = cursor.read_string()?;
    let stop_words = cursor.read_string()?;
    let stemming = cursor.read_string()?;
    let case_folding = cursor.read_bool()?;
    let accent_folding = cursor.read_bool()?;
    cursor.finish()?;
    Ok(FulltextIndexMetadata {
        version,
        built_generation,
        total_documents,
        documents_with_text,
        average_document_length,
        analyzer: crate::search::analyzer::AnalyzerConfig {
            name,
            tokenizer,
            case_folding,
            stop_words,
            stemming,
            accent_folding,
        },
    })
}

pub(super) fn encode_manifest(manifest: &FulltextManifest) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MANIFEST_MAGIC);
    for value in [
        u64::from(manifest.version),
        manifest.built_generation,
        u64::try_from(manifest.total_documents).unwrap_or(u64::MAX),
        u64::try_from(manifest.posting_terms).unwrap_or(u64::MAX),
        u64::try_from(manifest.document_count).unwrap_or(u64::MAX),
    ] {
        write_varint(value, &mut out);
    }
    write_varint(
        u64::try_from(manifest.terms.len()).unwrap_or(u64::MAX),
        &mut out,
    );
    for (term, integrity) in &manifest.terms {
        write_bytes(term.as_bytes(), &mut out);
        write_varint(
            u64::try_from(integrity.block_count).unwrap_or(u64::MAX),
            &mut out,
        );
        write_varint(
            u64::try_from(integrity.posting_count).unwrap_or(u64::MAX),
            &mut out,
        );
    }
    out
}

pub(super) fn decode_manifest(bytes: &[u8]) -> Result<FulltextManifest, CassieError> {
    let mut cursor = Cursor::new(bytes);
    cursor.expect(MANIFEST_MAGIC)?;
    let version = cursor.read_u32()?;
    if version != super::STATE_VERSION {
        return Err(CassieError::Parse(format!(
            "unsupported fulltext manifest version {version}"
        )));
    }
    let built_generation = cursor.read_varint()?;
    let total_documents = cursor.read_usize()?;
    let posting_terms = cursor.read_usize()?;
    let document_count = cursor.read_usize()?;
    let term_count = cursor.read_usize()?;
    let mut terms = BTreeMap::new();
    for _ in 0..term_count {
        let term = cursor.read_string()?;
        let integrity = super::FulltextTermIntegrity {
            block_count: cursor.read_usize()?,
            posting_count: cursor.read_usize()?,
        };
        if terms.insert(term, integrity).is_some() {
            return Err(CassieError::Parse(
                "duplicate fulltext manifest term".to_string(),
            ));
        }
    }
    let manifest = FulltextManifest {
        version,
        built_generation,
        total_documents,
        posting_terms,
        document_count,
        terms,
    };
    cursor.finish()?;
    Ok(manifest)
}

fn common_prefix_len(left: &[u8], right: &[u8]) -> usize {
    left.iter()
        .zip(right)
        .take_while(|(left, right)| left == right)
        .count()
}

fn write_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    write_varint(u64::try_from(bytes.len()).unwrap_or(u64::MAX), out);
    out.extend_from_slice(bytes);
}

fn write_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push(u8::try_from(value & 0x7f).expect("masked varint byte") | 0x80);
        value >>= 7;
    }
    out.push(u8::try_from(value).expect("final varint byte"));
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn expect(&mut self, expected: &[u8]) -> Result<(), CassieError> {
        if self.read_exact(expected.len())? == expected {
            Ok(())
        } else {
            Err(CassieError::Parse(
                "invalid fulltext binary record".to_string(),
            ))
        }
    }

    fn read_usize(&mut self) -> Result<usize, CassieError> {
        usize::try_from(self.read_varint()?)
            .map_err(|_| CassieError::Parse("fulltext integer overflow".to_string()))
    }

    fn read_u32(&mut self) -> Result<u32, CassieError> {
        u32::try_from(self.read_varint()?)
            .map_err(|_| CassieError::Parse("fulltext integer overflow".to_string()))
    }

    fn read_string(&mut self) -> Result<String, CassieError> {
        String::from_utf8(self.read_bytes()?.to_vec())
            .map_err(|error| CassieError::Parse(format!("invalid fulltext string: {error}")))
    }

    fn read_bool(&mut self) -> Result<bool, CassieError> {
        match self.read_exact(1)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(CassieError::Parse("invalid fulltext boolean".to_string())),
        }
    }

    fn read_f64(&mut self) -> Result<f64, CassieError> {
        let bytes: [u8; 8] = self
            .read_exact(8)?
            .try_into()
            .map_err(|_| CassieError::Parse("truncated fulltext float".to_string()))?;
        Ok(f64::from_bits(u64::from_be_bytes(bytes)))
    }

    fn read_bytes(&mut self) -> Result<&'a [u8], CassieError> {
        let len = self.read_usize()?;
        self.read_exact(len)
    }

    fn read_varint(&mut self) -> Result<u64, CassieError> {
        let mut value = 0_u64;
        for shift in (0..64).step_by(7) {
            let byte = self.read_exact(1)?[0];
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(CassieError::Parse("fulltext varint overflow".to_string()))
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], CassieError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| CassieError::Parse("fulltext offset overflow".to_string()))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| CassieError::Parse("truncated fulltext record".to_string()))?;
        self.offset = end;
        Ok(value)
    }

    fn finish(self) -> Result<(), CassieError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(CassieError::Parse(
                "trailing fulltext record bytes".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_roundtrip_prefix_compressed_postings() {
        // Arrange
        let postings = vec![
            PersistedFulltextPosting {
                document_id: "document-0001".to_string(),
                term_frequency: 2,
            },
            PersistedFulltextPosting {
                document_id: "document-0002".to_string(),
                term_frequency: 5,
            },
        ];

        // Act
        let encoded = encode_postings(&postings).expect("encode postings");
        let decoded = decode_postings(&encoded).expect("decode postings");

        // Assert
        assert_eq!(decoded, postings);
        assert_eq!(&encoded[..4], b"FTB1");
        assert!(encoded.len() <= MAX_POSTING_BLOCK_BYTES);
    }

    #[test]
    fn should_roundtrip_binary_document_stats() {
        // Arrange
        let stats = PersistedFulltextDocumentStats {
            doc_length: 4,
            term_counts: BTreeMap::from([("alpha".to_string(), 3), ("beta".to_string(), 1)]),
        };

        // Act
        let encoded = encode_document_stats(&stats);
        let decoded = decode_document_stats(&encoded).expect("decode document stats");

        // Assert
        assert_eq!(decoded, stats);
        assert_eq!(&encoded[..4], b"FTD1");
    }

    #[test]
    fn should_cap_large_posting_blocks() {
        // Arrange
        let postings = (0..30_000)
            .map(|index| PersistedFulltextPosting {
                document_id: format!("document-{index:08}"),
                term_frequency: 1,
            })
            .collect::<Vec<_>>();

        // Act
        let blocks = encode_posting_blocks(&postings).expect("posting blocks");
        let decoded = blocks
            .iter()
            .flat_map(|block| decode_postings(block).expect("decode block"))
            .collect::<Vec<_>>();

        // Assert
        assert!(blocks.len() > 1);
        assert!(blocks
            .iter()
            .all(|block| block.len() <= MAX_POSTING_BLOCK_BYTES));
        assert_eq!(decoded, postings);
    }
}
