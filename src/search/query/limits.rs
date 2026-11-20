//! Declared limits of the `'query'` full-text syntax.
//!
//! See `docs/fulltext-query-contract.md`. Structural limits are reported as
//! invalid-parameter errors (SQLSTATE `22023`); the expansion limits are
//! program-limit errors (SQLSTATE `54000`) and are never applied by silently
//! truncating a term list.

/// Maximum query text size in bytes.
pub const MAX_QUERY_BYTES: usize = 1024;
/// Maximum number of items (words, phrases, prefix and typo terms).
pub const MAX_QUERY_ITEMS: usize = 32;
/// Maximum nesting depth of parentheses and negations.
pub const MAX_QUERY_DEPTH: usize = 8;
/// Maximum span of one phrase, in token positions (stop-word gaps included).
pub const MAX_PHRASE_SPAN: usize = 16;
/// Maximum slop of a `"..."~N` phrase.
pub const MAX_PHRASE_SLOP: u32 = 8;
/// Minimum length of a prefix word, in characters.
pub const MIN_PREFIX_CHARS: usize = 2;
/// Minimum length of a typo word, in characters.
pub const MIN_TYPO_CHARS: usize = 4;
/// Maximum number of index terms one prefix or typo item may expand to.
pub const MAX_EXPANSIONS_PER_ITEM: usize = 128;
/// Maximum number of index terms one query may expand to in total.
pub const MAX_EXPANDED_TERMS: usize = 512;
