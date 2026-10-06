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
