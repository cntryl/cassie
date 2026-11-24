use super::{
    require_f64_metadata, require_numeric_evidence, require_numeric_metadata,
    required_metadata_string, PerformanceBenchmarkScenario,
};

pub(super) fn validate_query_evidence(
    summary: &serde_json::Value,
    allows_result_cache_hits: bool,
    scenario: Option<&PerformanceBenchmarkScenario>,
) -> Result<(), String> {
    let metadata = &summary["metadata"];
    for key in [
        "selected_access_path",
        "access_path_evidence_source",
        "fallback_reason",
        "fallback_evidence_source",
        "worker_leak_evidence_source",
    ] {
        if metadata[key].as_str().is_none_or(str::is_empty) {
            return Err(format!("benchmark summary missing metadata.{key}"));
        }
    }
    for key in [
        "result_cardinality",
        "worker_count",
        "configured_worker_count",
        "leaked_active_operator_workers",
        "setup_time_ns",
    ] {
        require_numeric_metadata(metadata, key)?;
    }
    for key in [
        "storage_reads",
        "candidate_count",
        "peak_query_memory_bytes",
        "execution_result_cache_hits",
    ] {
        require_numeric_evidence(summary, key)?;
    }
    if metadata.get("measurement_time_ns").is_some() {
        require_numeric_metadata(metadata, "measurement_time_ns")?;
    } else if summary["total_wall_clock_ns"]
        .as_u64()
        .is_none_or(|elapsed| elapsed == 0)
    {
        return Err(
            "benchmark summary missing measurement timing evidence in total_wall_clock_ns"
                .to_string(),
        );
    }
    let result_cache_hits = require_numeric_evidence(summary, "execution_result_cache_hits")?;
    if !allows_result_cache_hits && result_cache_hits != 0 {
        return Err("execution result cache warmed a timed benchmark query".to_string());
    }
    let failed_operations = require_numeric_metadata(metadata, "failed_operations")?;
    if failed_operations != 0 {
        return Err("benchmark summary recorded failed operations".to_string());
    }
    validate_worker_cleanup(metadata)?;
    if let Some(scenario) = scenario.filter(|scenario| scenario.requires_observed_query_evidence())
    {
        let access_source = metadata["access_path_evidence_source"]
            .as_str()
            .unwrap_or_default();
        if !matches!(access_source, "preflight" | "operation") {
            return Err(format!(
                "benchmark scenario {} requires observed selected_access_path evidence",
                scenario.scenario_id
            ));
        }
        if let Some(expected) = scenario.expected_selected_access_path() {
            let observed = metadata["selected_access_path"]
                .as_str()
                .unwrap_or_default();
            if observed != expected {
                return Err(format!(
                    "benchmark scenario {} selected access path mismatch: observed '{observed}', expected '{expected}'",
                    scenario.scenario_id
                ));
            }
            if access_source != "preflight" {
                return Err(format!(
                    "benchmark scenario {} requires preflight selected access path evidence",
                    scenario.scenario_id
                ));
            }
        }
        let fallback_source = metadata["fallback_evidence_source"]
            .as_str()
            .unwrap_or_default();
        if !matches!(
            fallback_source,
            "preflight" | "runtime_metrics" | "operation"
        ) {
            return Err(format!(
                "benchmark scenario {} requires observed fallback_reason evidence",
                scenario.scenario_id
            ));
        }
        if metadata["worker_leak_evidence_source"].as_str() != Some("runtime_metrics") {
            return Err(format!(
                "benchmark scenario {} requires observed worker-leak evidence",
                scenario.scenario_id
            ));
        }
    }
    if let Some(scenario) = scenario {
        validate_ann_release_evidence(scenario, metadata)?;
    }
    Ok(())
}

fn validate_worker_cleanup(metadata: &serde_json::Value) -> Result<(), String> {
    if require_numeric_metadata(metadata, "leaked_active_operator_workers")? != 0 {
        return Err("benchmark summary recorded leaked active operator workers".to_string());
    }
    Ok(())
}

fn validate_ann_release_evidence(
    scenario: &PerformanceBenchmarkScenario,
    metadata: &serde_json::Value,
) -> Result<(), String> {
    if scenario.expected_selected_access_path() == Some("hnsw") {
        validate_hnsw_release_evidence(scenario, metadata)?;
    } else if scenario.expected_selected_access_path() == Some("ivfflat") {
        validate_ivfflat_release_evidence(scenario, metadata)?;
    }
    Ok(())
}

pub fn validate_hnsw_release_evidence(
    scenario: &PerformanceBenchmarkScenario,
    metadata: &serde_json::Value,
) -> Result<(), String> {
    if scenario.expected_selected_access_path() != Some("hnsw") {
        return Err(format!(
            "benchmark scenario {} is not an HNSW release-evidence scenario",
            scenario.scenario_id
        ));
    }
    validate_ann_recall_floor(scenario, metadata)?;
    for (key, expected) in [
        ("exact_top_k", 20),
        ("vector_dimensions", 3),
        ("fixture_seed", 0),
        ("hnsw_m", 32),
        ("hnsw_ef_construction", 256),
        ("hnsw_ef_search", 256),
    ] {
        require_numeric_metadata_value(metadata, key, expected, scenario.scenario_id)?;
    }
    if required_metadata_string(metadata, "distance_metric")? != "l2" {
        return Err(format!(
            "benchmark scenario {} must record the declared l2 distance metric",
            scenario.scenario_id
        ));
    }
    Ok(())
}

pub fn validate_ivfflat_release_evidence(
    scenario: &PerformanceBenchmarkScenario,
    metadata: &serde_json::Value,
) -> Result<(), String> {
    if scenario.expected_selected_access_path() != Some("ivfflat") {
        return Err(format!(
            "benchmark scenario {} is not an IVFFlat release-evidence scenario",
            scenario.scenario_id
        ));
    }
    validate_ann_recall_floor(scenario, metadata)?;
    let (lists, probes, training_sample_size) = match scenario.benchmark {
        "tier3_system_query" => (64, 16, 4_096),
        "tier5_scaling_retrieval" => (16, 4, 1_024),
        owner => {
            return Err(format!(
                "benchmark scenario {} has unsupported IVFFlat owner {owner}",
                scenario.scenario_id
            ));
        }
    };
    for (key, expected) in [
        ("exact_top_k", 20),
        ("vector_dimensions", 3),
        ("fixture_seed", 0),
        ("ivfflat_lists", lists),
        ("ivfflat_probes", probes),
        ("ivfflat_training_sample_size", training_sample_size),
        ("ivfflat_training_seed", 42),
    ] {
        require_numeric_metadata_value(metadata, key, expected, scenario.scenario_id)?;
    }
    if required_metadata_string(metadata, "distance_metric")? != "l2" {
        return Err(format!(
            "benchmark scenario {} must record the declared l2 distance metric",
            scenario.scenario_id
        ));
    }
    if required_metadata_string(metadata, "filter_selectivity")? != "unfiltered" {
        return Err(format!(
            "benchmark scenario {} must record the declared unfiltered ANN profile",
            scenario.scenario_id
        ));
    }
    Ok(())
}

fn validate_ann_recall_floor(
    scenario: &PerformanceBenchmarkScenario,
    metadata: &serde_json::Value,
) -> Result<(), String> {
    let recall = require_recall_fraction_metadata(metadata, "recall_at_k")?;
    let floor = require_recall_fraction_metadata(metadata, "recall_floor")?;
    if (floor - 0.90).abs() > f64::EPSILON {
        return Err(format!(
            "benchmark scenario {} recall floor {floor} does not match the documented 0.9 floor",
            scenario.scenario_id
        ));
    }
    if recall < floor {
        return Err(format!(
            "benchmark scenario {} recall_at_k {recall} is below the {floor} floor",
            scenario.scenario_id
        ));
    }
    Ok(())
}

fn require_numeric_metadata_value(
    metadata: &serde_json::Value,
    key: &str,
    expected: u64,
    scenario_id: &str,
) -> Result<(), String> {
    let actual = require_numeric_metadata(metadata, key)?;
    if actual != expected {
        return Err(format!(
            "benchmark scenario {scenario_id} metadata.{key} {actual} does not match {expected}"
        ));
    }
    Ok(())
}

fn require_recall_fraction_metadata(
    metadata: &serde_json::Value,
    key: &str,
) -> Result<f64, String> {
    let value = require_f64_metadata(metadata, key)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(format!(
            "benchmark summary metadata.{key} must be between 0 and 1"
        ));
    }
    Ok(value)
}
