//! Declared scale and resource profiles plus sampling controls for the paired
//! heap-versus-column harness. Everything is selected through `BENCH_TIER3_*`
//! environment variables so default runs and CI keep the historical size.

use std::path::PathBuf;

const MIB: usize = 1024 * 1024;
const GIB: usize = 1024 * MIB;
const DEFAULT_WIDE_ROWS: usize = 100_000;

/// A declared fixture scale and resource envelope.
#[derive(Clone, Debug)]
pub struct Profile {
    pub name: &'static str,
    pub fact_rows: usize,
    pub query_budget: usize,
    pub native_total: usize,
    pub native_memtable: usize,
    /// Rows in the wide pair `f_row_wide`/`f_col_wide`.
    pub wide_rows: usize,
}

impl Profile {
    /// `BENCH_TIER3_STORAGE_MODES_PROFILE` selects `100k_256m` (default),
    /// `100k_2g` (the historical 2 GiB query budget) or `1m` (opt-in 1M rows).
    pub fn from_env() -> Self {
        let name = std::env::var("BENCH_TIER3_STORAGE_MODES_PROFILE")
            .unwrap_or_else(|_| "100k_256m".to_owned());
        let mut profile = match name.as_str() {
            "100k_256m" => Self {
                name: "storage_modes_100k_256m",
                fact_rows: 100_000,
                query_budget: 256 * MIB,
                native_total: 2 * GIB,
                native_memtable: 512 * MIB,
                wide_rows: DEFAULT_WIDE_ROWS,
            },
            "100k_2g" => Self {
                name: "storage_modes_100k_2g",
                fact_rows: 100_000,
                query_budget: 2 * GIB,
                native_total: 2 * GIB,
                native_memtable: 512 * MIB,
                wide_rows: DEFAULT_WIDE_ROWS,
            },
            "1m" => Self {
                name: "storage_modes_1m_4g",
                fact_rows: 1_000_000,
                query_budget: 4 * GIB,
                native_total: 8 * GIB,
                native_memtable: GIB,
                wide_rows: DEFAULT_WIDE_ROWS,
            },
            other => panic!("unknown BENCH_TIER3_STORAGE_MODES_PROFILE {other}"),
        };
        if let Some(rows) = env_usize("BENCH_TIER3_STORAGE_MODES_ROWS") {
            profile.fact_rows = rows;
        }
        profile.wide_rows = env_usize("BENCH_TIER3_STORAGE_MODES_WIDE_ROWS")
            .unwrap_or(profile.wide_rows)
            .min(profile.fact_rows);
        profile
    }
}

/// Opt-in gate behaviors.
#[derive(Clone, Copy, Debug)]
pub struct GateFlags {
    pub strict: bool,
    pub require_vector: bool,
}

/// Sampling, gating and output controls.
#[derive(Clone, Debug)]
pub struct Sampling {
    pub measurement: bool,
    pub gate: GateFlags,
    pub warmup: usize,
    pub samples: usize,
    pub control: bool,
    pub bootstrap: usize,
    pub filter: Vec<String>,
    pub out: PathBuf,
    pub footprint: bool,
    pub footprint_rows: usize,
    pub amplification_rows: usize,
}

impl Sampling {
    /// Measurement mode (`BENCH_TIER3_STORAGE_MODES_MEASUREMENT=1`) enforces at
    /// least 5 warmup and 30 measured samples; normal runs default to 1 and 5.
    pub fn from_env(profile: &Profile) -> Self {
        let measurement = env_flag("BENCH_TIER3_STORAGE_MODES_MEASUREMENT", false);
        let (warmup_default, samples_default) = if measurement { (5, 30) } else { (1, 5) };
        let mut warmup = env_usize("BENCH_TIER3_STORAGE_MODES_WARMUP").unwrap_or(warmup_default);
        let mut samples = env_usize("BENCH_TIER3_STORAGE_MODES_SAMPLES").unwrap_or(samples_default);
        if measurement {
            warmup = warmup.max(5);
            samples = samples.max(30);
        }
        Self {
            measurement,
            warmup,
            samples: samples.max(1),
            control: env_flag("BENCH_TIER3_STORAGE_MODES_CONTROL", measurement),
            bootstrap: env_usize("BENCH_TIER3_STORAGE_MODES_BOOTSTRAP").unwrap_or(2_000),
            gate: GateFlags {
                strict: env_flag("BENCH_TIER3_STORAGE_MODES_STRICT", false),
                require_vector: env_flag("BENCH_TIER3_STORAGE_MODES_REQUIRE_VECTOR", false),
            },
            filter: std::env::var("BENCH_TIER3_STORAGE_MODES_QUERIES").map_or_default(|list| {
                list.split(',')
                    .map(|item| item.trim().to_owned())
                    .filter(|item| !item.is_empty())
                    .collect()
            }),
            out: std::env::var_os("BENCH_TIER3_STORAGE_MODES_OUT").map_or_else(
                || PathBuf::from("target/storage_modes_paired.json"),
                PathBuf::from,
            ),
            footprint: env_flag("BENCH_TIER3_STORAGE_MODES_FOOTPRINT", true),
            footprint_rows: env_usize("BENCH_TIER3_STORAGE_MODES_FOOTPRINT_ROWS")
                .unwrap_or(profile.fact_rows.min(100_000)),
            amplification_rows: env_usize("BENCH_TIER3_STORAGE_MODES_AMPLIFICATION_ROWS")
                .unwrap_or(profile.fact_rows.min(20_000)),
        }
    }

    pub fn selects(&self, name: &str) -> bool {
        self.filter.is_empty() || self.filter.iter().any(|item| name.contains(item.as_str()))
    }
}

fn env_usize(name: &str) -> Option<usize> {
    std::env::var(name).ok().map(|value| {
        value
            .replace('_', "")
            .parse()
            .unwrap_or_else(|_| panic!("{name} must be an unsigned integer"))
    })
}

fn env_flag(name: &str, default: bool) -> bool {
    std::env::var(name).map_or(default, |value| {
        matches!(value.as_str(), "1" | "true" | "yes" | "on")
    })
}
