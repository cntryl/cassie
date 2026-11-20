//! Pure posting-block rules for in-place maintenance.
//!
//! A block belongs to the term's key range `[first id, next block's first id)`;
//! the first block of a term also owns every smaller id. Applying a document's
//! change rewrites only the block that owns it. A block that outgrows
//! [`codec::MAX_POSTING_BLOCK_BYTES`] splits into balanced blocks, and a block that
//! shrinks below [`MERGE_BELOW_BYTES`] absorbs its right neighbor when the two fit in
//! [`MERGE_LIMIT_BYTES`], so a term never fragments into many tiny blocks.

use super::{codec, CassieError, PersistedFulltextPosting};

/// A block smaller than this tries to absorb the following block.
pub(super) const MERGE_BELOW_BYTES: usize = codec::MAX_POSTING_BLOCK_BYTES / 16;
/// Merged blocks stay at or below this size so they do not split again at once.
pub(super) const MERGE_LIMIT_BYTES: usize = codec::MAX_POSTING_BLOCK_BYTES / 2;
const SPLIT_FILL_BYTES: usize = codec::MAX_POSTING_BLOCK_BYTES / 4 * 3;

/// One document's posting change for one term. `old` is the frequency the
/// maintained index must currently hold, `new` the frequency to hold afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PostingChange {
    pub(super) document_id: String,
    pub(super) old: Option<usize>,
    pub(super) new: Option<usize>,
}

/// Applies `changes` to sorted `postings` and returns the net posting-count
/// change.
///
/// # Errors
///
/// Returns an error when a change contradicts the stored postings, which means
/// the maintained index no longer matches its document statistics.
pub(super) fn apply_changes(
    postings: &mut Vec<PersistedFulltextPosting>,
    changes: &[PostingChange],
) -> Result<isize, CassieError> {
    let mut delta = 0_isize;
    for change in changes {
        let position = postings.binary_search_by(|posting| {
            posting
                .document_id
                .as_str()
                .cmp(change.document_id.as_str())
        });
        match (position, change.old, change.new) {
            (Ok(index), Some(old), Some(new)) if postings[index].term_frequency == old => {
                postings[index].term_frequency = new;
            }
            (Ok(index), Some(old), None) if postings[index].term_frequency == old => {
                postings.remove(index);
                delta -= 1;
            }
            (Err(index), None, Some(new)) => {
                postings.insert(
                    index,
                    PersistedFulltextPosting {
                        document_id: change.document_id.clone(),
                        term_frequency: new,
                    },
                );
                delta += 1;
            }
            _ => {
                return Err(CassieError::Parse(format!(
                    "fulltext postings disagree with document statistics for '{}'",
                    change.document_id
                )));
            }
        }
    }
    Ok(delta)
}

/// Splits `postings` into the blocks that replace one rewritten block: nothing
/// when empty, the postings themselves when they fit, otherwise balanced chunks.
pub(super) fn repack(postings: &[PersistedFulltextPosting]) -> Vec<&[PersistedFulltextPosting]> {
    if postings.is_empty() {
        return Vec::new();
    }
    let encoded = codec::encoded_postings_len(postings);
    if encoded <= codec::MAX_POSTING_BLOCK_BYTES {
        return vec![postings];
    }
    let pieces = encoded.div_ceil(SPLIT_FILL_BYTES);
    // Slack for the larger first entry of each chunk, which has no shared prefix.
    let target = encoded.div_ceil(pieces).saturating_add(64);
    codec::chunk_postings(postings, target)
}

/// Whether a rewritten block of `own_bytes` should absorb a neighbor of
/// `neighbor_bytes`.
pub(super) fn should_merge(own_bytes: usize, neighbor_bytes: usize) -> bool {
    own_bytes < MERGE_BELOW_BYTES && own_bytes + neighbor_bytes <= MERGE_LIMIT_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posting(id: &str, frequency: usize) -> PersistedFulltextPosting {
        PersistedFulltextPosting {
            document_id: id.to_string(),
            term_frequency: frequency,
        }
    }

    fn change(id: &str, old: Option<usize>, new: Option<usize>) -> PostingChange {
        PostingChange {
            document_id: id.to_string(),
            old,
            new,
        }
    }

    #[test]
    fn should_insert_update_and_remove_postings_in_id_order() {
        // Arrange
        let mut postings = vec![posting("b", 1), posting("d", 2)];
        let changes = [
            change("a", None, Some(3)),
            change("b", Some(1), Some(5)),
            change("d", Some(2), None),
        ];

        // Act
        let delta = apply_changes(&mut postings, &changes).expect("apply changes");

        // Assert
        assert_eq!(delta, 0);
        assert_eq!(postings, vec![posting("a", 3), posting("b", 5)]);
    }

    #[test]
    fn should_reject_a_change_that_contradicts_the_stored_postings() {
        // Arrange
        let mut postings = vec![posting("a", 1)];

        // Act
        let missing = apply_changes(&mut postings, &[change("b", Some(1), None)]);
        let stale_frequency = apply_changes(&mut postings, &[change("a", Some(9), Some(2))]);
        let duplicate = apply_changes(&mut postings, &[change("a", None, Some(2))]);

        // Assert
        assert!(missing.is_err());
        assert!(stale_frequency.is_err());
        assert!(duplicate.is_err());
    }

    #[test]
    fn should_split_an_oversized_block_into_bounded_balanced_blocks() {
        // Arrange
        let postings = (0..40_000)
            .map(|index| posting(&format!("document-{index:08}"), 1))
            .collect::<Vec<_>>();

        // Act
        let blocks = repack(&postings);
        let sizes = blocks
            .iter()
            .map(|block| codec::encoded_postings_len(block))
            .collect::<Vec<_>>();

        // Assert
        assert!(blocks.len() > 1);
        assert!(sizes
            .iter()
            .all(|size| *size <= codec::MAX_POSTING_BLOCK_BYTES));
        let largest = sizes.iter().max().copied().unwrap_or_default();
        let smallest = sizes.iter().min().copied().unwrap_or_default();
        assert!(smallest * 2 > largest, "blocks stay balanced: {sizes:?}");
        assert_eq!(blocks.concat(), postings);
    }

    #[test]
    fn should_keep_a_block_that_fits_and_drop_an_empty_one() {
        // Arrange
        let postings = vec![posting("a", 1), posting("b", 1)];

        // Act
        let kept = repack(&postings);
        let dropped = repack(&[]);

        // Assert
        assert_eq!(kept.len(), 1);
        assert!(dropped.is_empty());
    }

    #[test]
    fn should_merge_only_small_blocks_that_fit_together() {
        // Arrange
        let limit = MERGE_LIMIT_BYTES;

        // Act / Assert
        assert!(should_merge(100, 200));
        assert!(!should_merge(MERGE_BELOW_BYTES, 10));
        assert!(!should_merge(100, limit));
    }
}
