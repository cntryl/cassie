use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

pub use crate::performance_benchmarks::BenchmarkTier;
use crate::performance_benchmarks::{
    benchmark_scenarios, validate_scenario_contract, PerformanceBenchmarkScenario,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunCategory {
    Timed,
    Product,
    Diagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub package: String,
    pub target: String,
    pub tier: BenchmarkTier,
    pub category: RunCategory,
    pub required_features: Vec<String>,
}

impl Target {
    #[must_use]
    pub fn new(
        package: &str,
        target: &str,
        tier: BenchmarkTier,
        category: RunCategory,
        features: &[&str],
    ) -> Self {
        Self {
            package: package.to_owned(),
            target: target.to_owned(),
            tier,
            category,
            required_features: features
                .iter()
                .map(|feature| (*feature).to_owned())
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
    targets: Vec<CargoTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
    #[serde(default, rename = "required-features")]
    required_features: Vec<String>,
}

#[must_use]
/// Returns the registered Cargo targets represented by the scenario catalog and support tools.
///
/// # Panics
///
/// Panics when scenarios for one target disagree about its tier or run category.
pub fn registered_targets() -> Vec<Target> {
    let mut records = BTreeMap::<String, Target>::new();
    for scenario in benchmark_scenarios() {
        let name = scenario.benchmark;
        let tier = BenchmarkTier::from_number(scenario.declared_tier.number())
            .expect("scenario tier must be 1 through 6");
        let category = category_for_target(name);
        let features = if name == "tier3_replay_durability" {
            vec!["replay-benchmark-observation"]
        } else {
            Vec::new()
        };
        let target = Target {
            package: package_for_target(name).to_owned(),
            target: name.to_owned(),
            tier,
            category,
            required_features: features.into_iter().map(str::to_owned).collect(),
        };
        if let Some(previous) = records.insert(name.to_owned(), target.clone()) {
            assert_eq!(
                previous, target,
                "target registry metadata conflicts for {name}"
            );
        }
    }
    for (target, tier, category) in [
        (
            "tier4_pgwire_fixture_profile",
            BenchmarkTier::Tier4,
            RunCategory::Diagnostic,
        ),
        (
            "tier5_time_series_mixed",
            BenchmarkTier::Tier5,
            RunCategory::Timed,
        ),
        (
            "tier5_embedding_workloads",
            BenchmarkTier::Tier5,
            RunCategory::Product,
        ),
        (
            "tier3_product_graph",
            BenchmarkTier::Tier3,
            RunCategory::Product,
        ),
        (
            "tier3_product_column",
            BenchmarkTier::Tier3,
            RunCategory::Product,
        ),
        (
            "tier5_product_projection",
            BenchmarkTier::Tier5,
            RunCategory::Product,
        ),
        (
            "tier5_product_fulltext",
            BenchmarkTier::Tier5,
            RunCategory::Product,
        ),
        (
            "tier5_product_time_series",
            BenchmarkTier::Tier5,
            RunCategory::Product,
        ),
        (
            "relevance_cranfield",
            BenchmarkTier::Tier3,
            RunCategory::Diagnostic,
        ),
    ] {
        records.insert(
            target.to_owned(),
            Target::new("cassie-e2e", target, tier, category, &[]),
        );
    }
    records.into_values().collect()
}

fn package_for_target(target: &str) -> &'static str {
    match target {
        "tier1_hotpath_request_controls" => "cassie-core",
        "tier1_hotpath_local_embedding" | "tier2_subsystem_local_embeddings" => "cassie-embeddings",
        "tier1_hotpath_search_vector" | "tier1_hotpath_bm25" | "tier2_subsystem_search" => {
            "cassie-search"
        }
        "tier2_subsystem_parser" => "cassie-sql",
        "tier1_hotpath_vector_distance" | "tier2_subsystem_vector" => "cassie-vector",
        "tier1_hotpath_row_codec" | "tier1_hotpath_keys" | "tier2_subsystem_storage" => {
            "cassie-storage"
        }
        "tier1_hotpath_filter_projection"
        | "tier1_hotpath_predicates"
        | "tier1_hotpath_topk"
        | "tier1_hotpath_graph_paths"
        | "tier2_subsystem_binder"
        | "tier2_subsystem_column_scan"
        | "tier2_subsystem_executor"
        | "tier2_subsystem_generated_sources"
        | "tier2_subsystem_graph"
        | "tier2_subsystem_hybrid"
        | "tier2_subsystem_ingest"
        | "tier2_subsystem_plan_cache"
        | "tier2_subsystem_sql_planning" => "cassie-engine",
        "tier1_hotpath_pgwire" | "tier2_subsystem_protocol_handlers" => "cassie",
        _ => "cassie-e2e",
    }
}

fn category_for_target(target: &str) -> RunCategory {
    match target {
        "tier5_embedding_workloads" => RunCategory::Product,
        "tier3_replay_durability" | "tier4_pgwire_fixture_profile" | "relevance_cranfield" => {
            RunCategory::Diagnostic
        }
        _ => RunCategory::Timed,
    }
}

/// Checks exact package, target-kind, and feature placement against `cargo metadata --no-deps`.
///
/// # Errors
///
/// Returns an error for missing, duplicate, incorrectly placed, or feature-mismatched targets.
pub fn validate_cargo_metadata(targets: &[Target], metadata_json: &str) -> Result<(), String> {
    let metadata: CargoMetadata =
        serde_json::from_str(metadata_json).map_err(|error| error.to_string())?;
    let packages = metadata
        .packages
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect::<BTreeMap<_, _>>();
    let mut registered = BTreeSet::new();
    for target in targets {
        let key = (target.package.as_str(), target.target.as_str());
        if !registered.insert(key) {
            return Err(format!(
                "duplicate registry target {}/{}",
                target.package, target.target
            ));
        }
        let package = packages.get(target.package.as_str()).ok_or_else(|| {
            format!(
                "registry package {} is missing from Cargo metadata",
                target.package
            )
        })?;
        let matches = package
            .targets
            .iter()
            .filter(|candidate| {
                candidate.name == target.target && candidate.kind.iter().any(|kind| kind == "bench")
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(format!(
                "Cargo bench target {}/{} must be registered exactly once (found {})",
                target.package,
                target.target,
                matches.len()
            ));
        }
        let mut expected = target.required_features.clone();
        let mut actual = matches[0].required_features.clone();
        expected.sort();
        actual.sort();
        if expected != actual {
            return Err(format!(
                "required features mismatch for {}/{}: registry {:?}, Cargo {:?}",
                target.package, target.target, expected, actual
            ));
        }
    }
    for package in &metadata.packages {
        for candidate in &package.targets {
            if candidate.kind.iter().any(|kind| kind == "bench")
                && !registered.contains(&(package.name.as_str(), candidate.name.as_str()))
            {
                return Err(format!(
                    "Cargo bench target {}/{} is missing from the registry",
                    package.name, candidate.name
                ));
            }
        }
    }
    let scenarios = benchmark_scenarios().copied().collect::<Vec<_>>();
    validate_scenario_registry(targets, &scenarios)?;
    Ok(())
}

/// Verifies unique scenario IDs and exact target-to-tier ownership.
///
/// # Errors
///
/// Returns an error for duplicate IDs, invalid scenario contracts, missing targets, or tier mismatch.
pub fn validate_scenario_registry(
    targets: &[Target],
    scenarios: &[PerformanceBenchmarkScenario],
) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for scenario in scenarios {
        if !ids.insert(scenario.scenario_id) {
            return Err(format!("duplicate scenario id {}", scenario.scenario_id));
        }
        validate_scenario_contract(scenario)?;
        let target = targets
            .iter()
            .find(|target| target.target == scenario.benchmark)
            .ok_or_else(|| {
                format!(
                    "scenario {} target {} is missing from the target registry",
                    scenario.scenario_id, scenario.benchmark
                )
            })?;
        if target.tier.number() != scenario.declared_tier.number() {
            return Err(format!(
                "wrong-tier target registration for {}: target Tier {}, scenario Tier {}",
                scenario.benchmark,
                target.tier.number(),
                scenario.declared_tier.number()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
