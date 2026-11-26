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
    pub local: bool,
    /// Rows in the wide pair `f_row_wide`/`f_col_wide`.
    pub wide_rows: usize,
}

impl Profile {
    /// `BENCH_TIER3_STORAGE_MODES_PROFILE` selects `100k_256m` (default),
    /// `100k_256m_local` (same comparison on native local storage),
    /// `100k_2g` (historical budget), `1m` (opt-in memory scale) or
    /// `1m_local` (same rows and query budget on native local storage).
    ///
    /// # Panics
    /// Panics for an unknown profile or invalid or zero row counts.
    #[must_use]
    pub fn from_env(fixture: Fixture) -> Self {
        let name = std::env::var("BENCH_TIER3_STORAGE_MODES_PROFILE")
            .unwrap_or_else(|_| "100k_256m".to_owned());
        let mut profile = Self::named(&name);
        if let Some(rows) = env_usize("BENCH_TIER3_STORAGE_MODES_ROWS") {
            profile.fact_rows = rows;
        }
        profile.wide_rows =
            env_usize("BENCH_TIER3_STORAGE_MODES_WIDE_ROWS").unwrap_or(profile.wide_rows);
        if matches!(fixture, Fixture::All) {
            profile.wide_rows = profile.wide_rows.min(profile.fact_rows);
        }
        assert!(
            profile.fact_rows > 0,
            "narrow fixture must have at least one row"
        );
        assert!(
            profile.wide_rows > 0,
            "wide fixture must have at least one row"
        );
        profile
    }

    /// Resolves a named fixture profile without consulting process environment.
    ///
    /// # Panics
    /// Panics when the profile name is unknown.
    #[must_use]
    pub fn named(name: &str) -> Self {
        match name {
            "100k_256m" => Self {
                name: "storage_modes_100k_256m",
                fact_rows: 100_000,
                query_budget: 256 * MIB,
                native_total: 2 * GIB,
                native_memtable: 512 * MIB,
                local: false,
                wide_rows: DEFAULT_WIDE_ROWS,
            },
            "100k_256m_local" => Self {
                name: "storage_modes_100k_256m_local",
                fact_rows: 100_000,
                query_budget: 256 * MIB,
                native_total: 2 * GIB,
                native_memtable: 512 * MIB,
                local: true,
                wide_rows: DEFAULT_WIDE_ROWS,
            },
            "100k_2g" => Self {
                name: "storage_modes_100k_2g",
                fact_rows: 100_000,
                query_budget: 2 * GIB,
                native_total: 2 * GIB,
                native_memtable: 512 * MIB,
                local: false,
                wide_rows: DEFAULT_WIDE_ROWS,
            },
            "1m" | "1m_local" => Self {
                name: if name == "1m_local" {
                    "storage_modes_1m_4g_local"
                } else {
                    "storage_modes_1m_4g"
                },
                fact_rows: 1_000_000,
                query_budget: 4 * GIB,
                native_total: 8 * GIB,
                native_memtable: GIB,
                local: name == "1m_local",
                wide_rows: DEFAULT_WIDE_ROWS,
            },
            other => panic!("unknown BENCH_TIER3_STORAGE_MODES_PROFILE {other}"),
        }
    }

    #[must_use]
    pub fn is_advantage_profile(&self) -> bool {
        matches!(
            self.name,
            "storage_modes_100k_256m" | "storage_modes_100k_256m_local"
        ) && self.fact_rows == 100_000
    }

    #[must_use]
    pub fn native_limits(&self) -> Option<(usize, usize)> {
        (!self.local).then_some((self.native_total, self.native_memtable))
    }
}

/// Select which shared setup dependencies the paired harness will load.
#[derive(Clone, Copy, Debug)]
pub enum Fixture {
    All,
    Narrow,
    Wide,
}

impl Fixture {
    /// Selects the fixtures requested by `BENCH_TIER3_STORAGE_MODES_FIXTURE`.
    ///
    /// # Panics
    /// Panics when the fixture setting cannot be read or selects an unknown fixture.
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("BENCH_TIER3_STORAGE_MODES_FIXTURE").as_deref() {
            Err(std::env::VarError::NotPresent) | Ok("all") => Self::All,
            Ok("narrow") => Self::Narrow,
            Ok("wide") => Self::Wide,
            other => panic!("unsupported BENCH_TIER3_STORAGE_MODES_FIXTURE: {other:?}"),
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Narrow => "narrow",
            Self::Wide => "wide",
        }
    }

    #[must_use]
    pub const fn includes_narrow(self) -> bool {
        matches!(self, Self::All | Self::Narrow)
    }

    #[must_use]
    pub const fn includes_wide(self) -> bool {
        matches!(self, Self::All | Self::Wide)
    }

    #[must_use]
    pub fn includes_query(self, name: &str) -> bool {
        if name.starts_with("wide_") {
            self.includes_wide()
        } else {
            self.includes_narrow()
        }
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
    pub fixture: Fixture,
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
    ///
    /// # Panics
    /// Panics when a numeric sampling setting is invalid.
    #[must_use]
    pub fn from_env(profile: &Profile, fixture: Fixture) -> Self {
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
            fixture,
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

    #[must_use]
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
