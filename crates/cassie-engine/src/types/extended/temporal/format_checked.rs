//! Bounded string helpers for cancellation-aware temporal formatting.

pub(super) const CHECK_INTERVAL: usize = 128;

pub(super) fn push_str_with_check<E>(
    output: &mut String,
    text: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<(), E> {
    let mut start = 0;
    let mut chunk_chars = 0;
    for (byte_index, _) in text.char_indices() {
        if chunk_chars == CHECK_INTERVAL {
            output.push_str(&text[start..byte_index]);
            check()?;
            start = byte_index;
            chunk_chars = 0;
        }
        chunk_chars += 1;
    }
    output.push_str(&text[start..]);
    check()
}

pub(super) fn literal_is_whitespace_with_check<E>(
    text: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<bool, E> {
    for (index, character) in text.chars().enumerate() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            check()?;
        }
        if !character.is_whitespace() {
            return Ok(false);
        }
    }
    Ok(true)
}
