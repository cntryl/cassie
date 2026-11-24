//! The per-session pseudo-random sequence behind `random()` and `setseed()`.

/// A `xoshiro256**` generator; `setseed` makes the sequence repeatable.
#[derive(Debug, Clone)]
pub(crate) struct SessionRandom {
    state: [u64; 4],
}

/// `splitmix64`, which expands one seed word into the generator state.
fn split_mix(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut value = *seed;
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

impl SessionRandom {
    fn from_seed(mut seed: u64) -> Self {
        Self {
            state: [
                split_mix(&mut seed),
                split_mix(&mut seed),
                split_mix(&mut seed),
                split_mix(&mut seed),
            ],
        }
    }

    /// A generator seeded from the operating system.
    pub(crate) fn from_entropy() -> Self {
        let mut bytes = [0u8; 8];
        if getrandom::fill(&mut bytes).is_err() {
            bytes = u64::from(std::process::id()).to_le_bytes();
        }
        Self::from_seed(u64::from_le_bytes(bytes))
    }

    /// `setseed(seed)` for a seed in `[-1, 1]`.
    pub(crate) fn reseed(&mut self, seed: f64) {
        *self = Self::from_seed(seed.to_bits());
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let result = self.state[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let shifted = self.state[1] << 17;
        self.state[2] ^= self.state[0];
        self.state[3] ^= self.state[1];
        self.state[1] ^= self.state[2];
        self.state[0] ^= self.state[3];
        self.state[2] ^= shifted;
        self.state[3] = self.state[3].rotate_left(45);
        result
    }

    /// A uniform double in `[0, 1)`.
    pub(crate) fn next_f64(&mut self) -> f64 {
        let bits = (self.next_u64() >> 11) | 0x3FF0_0000_0000_0000;
        f64::from_bits(bits) - 1.0
    }
}

impl Default for SessionRandom {
    fn default() -> Self {
        Self::from_entropy()
    }
}
