use crate::types::Value;

#[derive(Debug)]
pub(super) enum Validity {
    AllValid,
    AllNull,
    Bitmap(Vec<u64>),
}

impl Validity {
    pub(super) fn from_values(values: &[Value]) -> Self {
        if values.iter().all(Value::is_null) {
            return Self::AllNull;
        }
        if values.iter().all(|value| !value.is_null()) {
            return Self::AllValid;
        }
        let mut words = vec![0; values.len().div_ceil(64)];
        for (lane, value) in values.iter().enumerate() {
            if !value.is_null() {
                words[lane / 64] |= 1_u64 << (lane % 64);
            }
        }
        Self::Bitmap(words)
    }

    pub(super) fn is_valid(&self, lane: usize) -> bool {
        match self {
            Self::AllValid => true,
            Self::AllNull => false,
            Self::Bitmap(words) => words[lane / 64] & (1_u64 << (lane % 64)) != 0,
        }
    }
}

impl Validity {
    /// Builds validity from little-endian bit words for `len` lanes.
    ///
    /// Returns `None` when the word count does not match `len` or a bit at or
    /// beyond `len` is set.
    pub(super) fn from_words(words: &[u64], len: usize) -> Option<Self> {
        if words.len() != len.div_ceil(64) {
            return None;
        }
        let tail = len % 64;
        if tail != 0 && words[words.len() - 1] >> tail != 0 {
            return None;
        }
        let set = words
            .iter()
            .map(|word| usize::try_from(word.count_ones()).unwrap_or(usize::MAX))
            .sum::<usize>();
        Some(if set == len {
            Self::AllValid
        } else if set == 0 {
            Self::AllNull
        } else {
            Self::Bitmap(words.to_vec())
        })
    }

    pub(super) fn from_flags(flags: &[bool]) -> Self {
        if flags.iter().all(|valid| *valid) {
            return Self::AllValid;
        }
        if flags.iter().all(|valid| !*valid) {
            return Self::AllNull;
        }
        let mut words = vec![0; flags.len().div_ceil(64)];
        for (lane, valid) in flags.iter().enumerate() {
            if *valid {
                words[lane / 64] |= 1_u64 << (lane % 64);
            }
        }
        Self::Bitmap(words)
    }
}
