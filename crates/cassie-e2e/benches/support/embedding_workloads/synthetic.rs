//! The deterministic synthetic control (#1365): clustered Gaussian unit vectors.
//!
//! The generator uses only integer arithmetic plus IEEE `f32` `+`, `*`, `/` and `sqrt`, so every
//! platform produces the same bytes. Its parameters live in the manifest
//! (`synthetic_control`) and the expected SHA-256 of each output is recorded there too.

use sha2::{Digest, Sha256};

use super::manifest::SyntheticControl;
use super::vector_file::hex;

/// `SplitMix64`, a small and fully specified generator.
struct Stream(u64);

impl Stream {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    fn uniform(&mut self) -> f32 {
        let high = u16::try_from(self.next_u64() >> 48).unwrap_or(u16::MAX);
        f32::from(high) / 65_536.0
    }

    /// Irwin-Hall approximation of a standard normal: twelve uniforms minus six.
    fn gaussian(&mut self) -> f32 {
        (0..12).map(|_| self.uniform()).sum::<f32>() - 6.0
    }

    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound.max(1)).unwrap_or(u64::MAX);
        usize::try_from(self.next_u64() % bound).unwrap_or(0)
    }
}

fn normalize(values: &[f32]) -> Vec<f32> {
    let norm = values
        .iter()
        .fold(0.0_f32, |sum, value| sum + value * value)
        .sqrt()
        .max(1e-12);
    values.iter().map(|value| value / norm).collect()
}

fn centers(control: &SyntheticControl) -> Vec<Vec<f32>> {
    let mut stream = Stream(control.center_stream);
    (0..control.clusters)
        .map(|_| {
            let raw: Vec<f32> = (0..control.dimensions).map(|_| stream.gaussian()).collect();
            normalize(&raw)
        })
        .collect()
}

/// Per-component noise whose total length is `noise_milli / 1000` of a unit center.
fn component_noise(control: &SyntheticControl) -> f32 {
    let scale = f32::from(u16::try_from(control.dimensions).unwrap_or(u16::MAX)).sqrt();
    f32::from(control.noise_milli) / 1000.0 / scale
}

fn rows(control: &SyntheticControl, centers: &[Vec<f32>], stream: u64, count: usize) -> Vec<f32> {
    let mut stream = Stream(stream);
    let noise = component_noise(control);
    let mut out = Vec::with_capacity(count * control.dimensions);
    for _ in 0..count {
        let center = &centers[stream.below(control.clusters)];
        let raw: Vec<f32> = center
            .iter()
            .map(|component| component + noise * stream.gaussian())
            .collect();
        out.extend(normalize(&raw));
    }
    out
}

/// The generated document and query vectors of one synthetic control set.
pub struct SyntheticVectors {
    pub documents: Vec<f32>,
    pub queries: Vec<f32>,
}

/// Generates `documents` document rows and `queries` query rows.
#[must_use]
pub fn generate(control: &SyntheticControl, documents: usize, queries: usize) -> SyntheticVectors {
    let centers = centers(control);
    SyntheticVectors {
        documents: rows(control, &centers, control.document_stream, documents),
        queries: rows(control, &centers, control.query_stream, queries),
    }
}

/// Lowercase hex SHA-256 of the little-endian bytes of `values`.
#[must_use]
pub fn checksum(values: &[f32]) -> String {
    let mut hasher = Sha256::new();
    for value in values {
        hasher.update(value.to_le_bytes());
    }
    hex(&hasher.finalize())
}
