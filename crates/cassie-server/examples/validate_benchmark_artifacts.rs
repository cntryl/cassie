use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cntryl_stress::artifact::{QualityClass, RunProfile, StressRun, TrustClass};

const COLUMN_SHARD_SUITE: &str = "tier2_subsystem_column_scan";
const COLUMN_SCENARIOS: [&str; 8] = [
    "perf.column.selective_encoded_scan.2k",
    "perf.column.selective_plain_scan_baseline.2k",
    "perf.column.incompressible_adaptive_scan.2k",
    "perf.column.incompressible_plain_scan_baseline.2k",
    "perf.column.alp_selective_scan.2k",
    "perf.column.alp_plain_scan_baseline.2k",
    "perf.column.fsst_selective_scan.2k",
    "perf.column.fsst_plain_scan_baseline.2k",
];

#[derive(Clone, Copy)]
struct RelativeP95Gate {
    candidate: &'static str,
    baseline: &'static str,
    maximum_ratio: f64,
}

const COLUMN_P95_GATES: [RelativeP95Gate; 4] = [
    RelativeP95Gate {
        candidate: "perf.column.selective_encoded_scan.2k",
        baseline: "perf.column.selective_plain_scan_baseline.2k",
        maximum_ratio: 0.85,
    },
    RelativeP95Gate {
        candidate: "perf.column.incompressible_adaptive_scan.2k",
        baseline: "perf.column.incompressible_plain_scan_baseline.2k",
        maximum_ratio: 1.05,
    },
    RelativeP95Gate {
        candidate: "perf.column.alp_selective_scan.2k",
        baseline: "perf.column.alp_plain_scan_baseline.2k",
        maximum_ratio: 1.05,
    },
    RelativeP95Gate {
        candidate: "perf.column.fsst_selective_scan.2k",
        baseline: "perf.column.fsst_plain_scan_baseline.2k",
        maximum_ratio: 1.05,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EvidenceCellStatus {
    GateCandidate,
    Diagnostic,
    Experimental,
    Invalid,
    Skipped,
    Unavailable,
}

impl EvidenceCellStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::GateCandidate => "GATE-CANDIDATE",
            Self::Diagnostic => "DIAGNOSTIC",
            Self::Experimental => "EXPERIMENTAL",
            Self::Invalid => "INVALID",
            Self::Skipped => "SKIPPED",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

enum ValidationInput {
    Artifact(PathBuf),
    Skipped(String),
    Unavailable(String),
}

struct ValidationPlan {
    expected_source_commit: String,
    expected_run_id: String,
    inputs: Vec<ValidationInput>,
}

struct ArtifactRow {
    scenario_id: String,
    status: EvidenceCellStatus,
    trust: Option<String>,
    quality: Option<String>,
    measured_samples: Option<usize>,
}

struct ArtifactReport {
    path: PathBuf,
    suite: String,
    profile: String,
    cpu_identity: String,
    os: String,
    source_commit: String,
    run_id: String,
    deployment_profile: String,
    rows: Vec<ArtifactRow>,
    errors: Vec<String>,
}

fn evidence_cell_status(
    trust: Option<TrustClass>,
    quality: Option<QualityClass>,
    correctness_passed: bool,
) -> EvidenceCellStatus {
    match trust {
        Some(TrustClass::Experimental) => EvidenceCellStatus::Experimental,
        Some(TrustClass::Invalid) => EvidenceCellStatus::Invalid,
        Some(TrustClass::Gate)
            if correctness_passed
                && matches!(
                    quality,
                    Some(QualityClass::Authoritative | QualityClass::Acceptable)
                ) =>
        {
            EvidenceCellStatus::GateCandidate
        }
        Some(TrustClass::Diagnostic | TrustClass::Gate) => EvidenceCellStatus::Diagnostic,
        None | Some(_) => EvidenceCellStatus::Unavailable,
    }
}

fn inspect_artifact(
    path: &Path,
    expected_source_commit: &str,
    expected_run_id: &str,
) -> Result<ArtifactReport, String> {
    let run = StressRun::load(path)
        .map_err(|error| format!("cannot load {}: {error}", path.display()))?;
    let mut errors = Vec::new();
    if let Err(error) = run.validate_canonical_evidence() {
        errors.push(error);
    }
    if run.benchmark_specs.is_empty() {
        errors.push("artifact contains no benchmark specs".to_string());
    }
    if run.summaries.is_empty() {
        errors.push("artifact contains no benchmark summaries".to_string());
    }
    if run.run_profile != RunProfile::Release {
        errors.push(format!(
            "artifact profile is {}, expected release",
            run.run_profile
        ));
    }
    if run.environment.build_profile != "release" {
        errors.push(format!(
            "artifact build profile is {:?}, expected release",
            run.environment.build_profile
        ));
    }
    validate_column_shard(&run, &mut errors);

    let source_commit = run
        .environment
        .git_commit
        .clone()
        .filter(|commit| !commit.is_empty())
        .unwrap_or_else(|| {
            errors.push("artifact is missing its source commit".to_string());
            "unavailable".to_string()
        });
    if source_commit != expected_source_commit {
        errors.push(format!(
            "artifact source commit {source_commit:?} does not match expected {expected_source_commit:?}"
        ));
    }
    let run_id = required_metadata(&run, "run_id", &mut errors);
    if run_id != expected_run_id {
        errors.push(format!(
            "artifact run ID {run_id:?} does not match expected {expected_run_id:?}"
        ));
    }
    let deployment_profile = required_metadata(&run, "deployment_profile_id", &mut errors);
    match expected_linux_os_for_profile(&deployment_profile) {
        Some(expected_os) if run.environment.os == expected_os => {}
        Some(expected_os) => errors.push(format!(
            "artifact OS {:?} does not match deployment profile {deployment_profile:?} ({expected_os})",
            run.environment.os
        )),
        None => errors.push(format!(
            "deployment profile {deployment_profile:?} is not a supported native Linux disk profile"
        )),
    }
    let rows = run
        .benchmark_specs
        .iter()
        .map(|spec| {
            let summary = run
                .summaries
                .iter()
                .find(|summary| summary.benchmark_id == spec.id);
            ArtifactRow {
                scenario_id: spec
                    .metadata
                    .get("scenario_id")
                    .cloned()
                    .unwrap_or_else(|| "unavailable".to_string()),
                status: summary.map_or(EvidenceCellStatus::Unavailable, |summary| {
                    evidence_cell_status(
                        Some(summary.trust_class),
                        Some(summary.quality),
                        summary.correctness.passed,
                    )
                }),
                trust: summary.map(|summary| summary.trust_class.to_string()),
                quality: summary.map(|summary| summary.quality.to_string()),
                measured_samples: summary.map(|summary| summary.measured_samples),
            }
        })
        .collect();

    Ok(ArtifactReport {
        path: path.to_path_buf(),
        suite: run.suite,
        profile: run.run_profile.to_string(),
        cpu_identity: run.environment.cpu_model,
        os: run.environment.os,
        source_commit,
        run_id,
        deployment_profile,
        rows,
        errors,
    })
}

fn validate_column_shard(run: &StressRun, errors: &mut Vec<String>) {
    validate_column_shard_shape(run, errors);
    validate_column_scenarios(run, errors);
    validate_column_p95_gates(run, errors);
}

fn validate_column_shard_shape(run: &StressRun, errors: &mut Vec<String>) {
    if run.suite != COLUMN_SHARD_SUITE {
        errors.push(format!(
            "artifact suite is {:?}, expected {COLUMN_SHARD_SUITE:?}",
            run.suite
        ));
    }
    if run.benchmark_specs.len() != COLUMN_SCENARIOS.len() {
        errors.push(format!(
            "column shard has {} specs, expected {}",
            run.benchmark_specs.len(),
            COLUMN_SCENARIOS.len()
        ));
    }
    if run.summaries.len() != COLUMN_SCENARIOS.len() {
        errors.push(format!(
            "column shard has {} summaries, expected {}",
            run.summaries.len(),
            COLUMN_SCENARIOS.len()
        ));
    }
}

fn validate_column_scenarios(run: &StressRun, errors: &mut Vec<String>) {
    for scenario in COLUMN_SCENARIOS {
        let matching_specs = run
            .benchmark_specs
            .iter()
            .filter(|spec| {
                spec.metadata
                    .get("scenario_id")
                    .is_some_and(|id| id == scenario)
            })
            .count();
        if matching_specs != 1 {
            errors.push(format!(
                "column shard scenario {scenario:?} has {matching_specs} specs, expected one"
            ));
        }

        let matching_summaries = run
            .summaries
            .iter()
            .filter(|summary| {
                summary
                    .metadata
                    .get("scenario_id")
                    .is_some_and(|id| id == scenario)
            })
            .collect::<Vec<_>>();
        if matching_summaries.len() != 1 {
            errors.push(format!(
                "column shard scenario {scenario:?} has {} summaries, expected one",
                matching_summaries.len()
            ));
            continue;
        }

        let Some(summary) = matching_summaries.first().copied() else {
            continue;
        };
        if summary.trust_class != TrustClass::Gate {
            errors.push(format!(
                "column shard scenario {scenario:?} has trust {}, expected gate",
                summary.trust_class
            ));
        }
        if !matches!(
            summary.quality,
            QualityClass::Authoritative | QualityClass::Acceptable
        ) {
            errors.push(format!(
                "column shard scenario {scenario:?} has unusable quality {}",
                summary.quality
            ));
        }
        if !summary.correctness.passed {
            errors.push(format!(
                "column shard scenario {scenario:?} failed correctness"
            ));
        }
    }

    for summary in &run.summaries {
        match summary.metadata.get("scenario_id") {
            Some(scenario) if COLUMN_SCENARIOS.contains(&scenario.as_str()) => {}
            Some(scenario) => errors.push(format!(
                "column shard contains unexpected scenario {scenario:?}"
            )),
            None => errors.push(format!(
                "column shard summary {:?} is missing scenario_id",
                summary.benchmark_id
            )),
        }
    }
}

fn validate_column_p95_gates(run: &StressRun, errors: &mut Vec<String>) {
    for gate in COLUMN_P95_GATES {
        let candidate = summary_for_scenario(run, gate.candidate);
        let baseline = summary_for_scenario(run, gate.baseline);
        let (Some(candidate), Some(baseline)) = (candidate, baseline) else {
            continue;
        };
        let candidate_p95 = p95_nanoseconds(candidate, errors);
        let baseline_p95 = p95_nanoseconds(baseline, errors);
        let (Some(candidate_p95), Some(baseline_p95)) = (candidate_p95, baseline_p95) else {
            continue;
        };
        if baseline_p95 <= 0.0 {
            errors.push(format!(
                "relative p95 baseline {:?} must be positive",
                gate.baseline
            ));
            continue;
        }
        let ratio = candidate_p95 / baseline_p95;
        if ratio > gate.maximum_ratio {
            errors.push(format!(
                "relative p95 gate failed: {:?} is {ratio:.3}x {:?}; maximum {:.3}x",
                gate.candidate, gate.baseline, gate.maximum_ratio
            ));
        }
    }
}

fn summary_for_scenario<'a>(
    run: &'a StressRun,
    scenario: &str,
) -> Option<&'a cntryl_stress::artifact::BenchmarkSummary> {
    run.summaries.iter().find(|summary| {
        summary
            .metadata
            .get("scenario_id")
            .is_some_and(|id| id == scenario)
    })
}

fn p95_nanoseconds(
    summary: &cntryl_stress::artifact::BenchmarkSummary,
    errors: &mut Vec<String>,
) -> Option<f64> {
    let value = summary
        .ns_per_op
        .as_ref()
        .or(summary.stats.as_ref())
        .map(|stats| stats.p95)
        .filter(|value| value.is_finite() && *value >= 0.0);
    if value.is_none() {
        errors.push(format!(
            "column shard scenario {:?} has no finite p95 latency",
            summary
                .metadata
                .get("scenario_id")
                .map_or(summary.benchmark_id.as_str(), String::as_str)
        ));
    }
    value
}

fn expected_linux_os_for_profile(profile: &str) -> Option<&'static str> {
    match profile {
        "native-linux-amd64-disk" => Some("linux x86_64"),
        "native-linux-arm64-disk" => Some("linux aarch64"),
        _ => None,
    }
}

fn required_metadata(run: &StressRun, key: &str, errors: &mut Vec<String>) -> String {
    run.metadata
        .get(key)
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| {
            errors.push(format!("artifact is missing metadata.{key}"));
            "unavailable".to_string()
        })
}

fn print_report(report: &ArtifactReport) {
    println!(
        "artifact={} suite={} profile={} os={} cpu={} source_commit={} run_id={} deployment_profile={}",
        report.path.display(),
        report.suite,
        report.profile,
        report.os,
        report.cpu_identity,
        report.source_commit,
        report.run_id,
        report.deployment_profile
    );
    if report.rows.is_empty() {
        println!("  UNAVAILABLE: no benchmark specs are recorded");
    }
    for row in &report.rows {
        println!(
            "  {}: {} trust={} quality={} measured_samples={}",
            row.scenario_id,
            row.status.as_str(),
            row.trust.as_deref().unwrap_or("unavailable"),
            row.quality.as_deref().unwrap_or("unavailable"),
            row.measured_samples
                .map_or_else(|| "unavailable".to_string(), |count| count.to_string())
        );
    }
    if report.errors.is_empty() {
        println!("SELECTED SHARD PASS");
    } else {
        for error in &report.errors {
            eprintln!("  validation error: {error}");
        }
        println!("SELECTED SHARD FAIL");
    }
}

fn parse_plan(
    arguments: impl Iterator<Item = std::ffi::OsString>,
) -> Result<ValidationPlan, String> {
    let mut arguments = arguments;
    let mut inputs = Vec::new();
    let mut expected_source_commit = None;
    let mut expected_run_id = None;
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--expected-source-commit") => {
                if expected_source_commit.is_some() {
                    return Err("--expected-source-commit may be supplied only once".to_string());
                }
                let commit = arguments
                    .next()
                    .ok_or_else(|| "--expected-source-commit requires a value".to_string())?
                    .into_string()
                    .map_err(|_| "expected source commit must be valid UTF-8".to_string())?;
                if !matches!(commit.len(), 40 | 64)
                    || !commit.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(
                        "expected source commit must be a full 40- or 64-digit hexadecimal ID"
                            .to_string(),
                    );
                }
                expected_source_commit = Some(commit);
            }
            Some("--expected-run-id") => {
                if expected_run_id.is_some() {
                    return Err("--expected-run-id may be supplied only once".to_string());
                }
                let run_id = arguments
                    .next()
                    .ok_or_else(|| "--expected-run-id requires a value".to_string())?
                    .into_string()
                    .map_err(|_| "expected run ID must be valid UTF-8".to_string())?;
                if run_id.is_empty() || run_id.trim() != run_id {
                    return Err("expected run ID must be non-empty and trimmed".to_string());
                }
                expected_run_id = Some(run_id);
            }
            Some("--skipped-cell") => {
                let cell = arguments
                    .next()
                    .ok_or_else(|| "--skipped-cell requires a cell ID".to_string())?
                    .into_string()
                    .map_err(|_| "cell IDs must be valid UTF-8".to_string())?;
                if cell.is_empty() {
                    return Err("--skipped-cell requires a non-empty cell ID".to_string());
                }
                inputs.push(ValidationInput::Skipped(cell));
            }
            Some("--unavailable-cell") => {
                let cell = arguments
                    .next()
                    .ok_or_else(|| "--unavailable-cell requires a cell ID".to_string())?
                    .into_string()
                    .map_err(|_| "cell IDs must be valid UTF-8".to_string())?;
                if cell.is_empty() {
                    return Err("--unavailable-cell requires a non-empty cell ID".to_string());
                }
                inputs.push(ValidationInput::Unavailable(cell));
            }
            _ => inputs.push(ValidationInput::Artifact(PathBuf::from(argument))),
        }
    }
    if inputs.is_empty() {
        return Err("at least one artifact or cell status is required".to_string());
    }
    let expected_source_commit =
        expected_source_commit.ok_or_else(|| "--expected-source-commit is required".to_string())?;
    let expected_run_id =
        expected_run_id.ok_or_else(|| "--expected-run-id is required".to_string())?;
    Ok(ValidationPlan {
        expected_source_commit,
        expected_run_id,
        inputs,
    })
}

fn has_selected_artifact(inputs: &[ValidationInput]) -> bool {
    inputs
        .iter()
        .any(|input| matches!(input, ValidationInput::Artifact(_)))
}

fn main() -> ExitCode {
    let plan = match parse_plan(std::env::args_os().skip(1)) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!(
                "usage: cargo run --locked --example validate_benchmark_artifacts -- --expected-source-commit <full-sha> --expected-run-id <id> <artifact.json>... [--skipped-cell <id>]... [--unavailable-cell <id>]...\n{error}"
            );
            return ExitCode::FAILURE;
        }
    };
    if !has_selected_artifact(&plan.inputs) {
        eprintln!("no selected artifact was supplied for canonical validation");
        return ExitCode::FAILURE;
    }

    let mut failed = false;
    for input in plan.inputs {
        match input {
            ValidationInput::Artifact(path) => {
                match inspect_artifact(&path, &plan.expected_source_commit, &plan.expected_run_id) {
                    Ok(report) => {
                        if !report.errors.is_empty() {
                            failed = true;
                        }
                        print_report(&report);
                    }
                    Err(error) => {
                        eprintln!("CANONICAL FAIL {}: {error}", path.display());
                        failed = true;
                    }
                }
            }
            ValidationInput::Skipped(cell) => {
                println!("cell={cell}: {}", EvidenceCellStatus::Skipped.as_str());
            }
            ValidationInput::Unavailable(cell) => {
                println!("cell={cell}: {}", EvidenceCellStatus::Unavailable.as_str());
                failed = true;
            }
        }
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{
        evidence_cell_status, has_selected_artifact, inspect_artifact, parse_plan,
        EvidenceCellStatus, ValidationInput,
    };
    use cntryl_stress::{
        artifact::{QualityClass, RunProfile, StressRun, TrustClass},
        StressRunner, StressRunnerConfig,
    };

    static NEXT_ARTIFACT_FILE: AtomicU64 = AtomicU64::new(0);

    struct TestArtifactFile(PathBuf);

    impl TestArtifactFile {
        fn new() -> Self {
            let sequence = NEXT_ARTIFACT_FILE.fetch_add(1, Ordering::Relaxed);
            Self(std::env::temp_dir().join(format!(
                "cassie-selected-shard-{}-{sequence}.json",
                std::process::id()
            )))
        }
    }

    impl Drop for TestArtifactFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn current_stress_run(fail_selective_gate: bool) -> StressRun {
        use std::time::Duration;

        use super::COLUMN_SCENARIOS;

        let config = StressRunnerConfig::for_profile(RunProfile::Release)
            .samples(10)
            .warmup_samples(0)
            .cooldown_samples(0)
            .progress(false);
        let mut runner = StressRunner::with_config("tier2_subsystem_column_scan", config);
        runner.reporters(Vec::new());
        for (index, scenario) in COLUMN_SCENARIOS.into_iter().enumerate() {
            let duration_ms = match index {
                0 if fail_selective_gate => 9,
                0 => 4,
                _ => 8,
            };
            runner.run(scenario, |context| {
                context.metadata("scenario_id", scenario);
                context.record_external("identity", Duration::from_millis(duration_ms), 1);
            });
        }
        let mut run = runner.finish();
        run.environment.cpu_model = "fixture CPU identity".to_string();
        for sample in &mut run.samples {
            sample.environment.cpu_model = "fixture CPU identity".to_string();
            sample.environment.os = "linux aarch64".to_string();
            sample.environment.build_profile = "release".to_string();
        }
        run.environment.os = "linux aarch64".to_string();
        run.environment.build_profile = "release".to_string();
        run.environment.git_commit = Some("0123456789abcdef0123456789abcdef01234567".to_string());
        run.metadata
            .insert("run_id".to_string(), "selected-shard-test".to_string());
        run.metadata.insert(
            "deployment_profile_id".to_string(),
            "native-linux-arm64-disk".to_string(),
        );
        run
    }

    #[test]
    fn should_keep_diagnostic_rows_out_of_gate_candidates() {
        // Arrange
        let diagnostic = Some(TrustClass::Diagnostic);
        let authoritative = Some(QualityClass::Authoritative);

        // Act
        let status = evidence_cell_status(diagnostic, authoritative, true);

        // Assert
        assert_eq!(status, EvidenceCellStatus::Diagnostic);
    }

    #[test]
    fn should_mark_missing_rows_unavailable_without_calling_them_skipped() {
        // Arrange
        let trust = None;
        let quality = None;

        // Act
        let status = evidence_cell_status(trust, quality, false);

        // Assert
        assert_eq!(status, EvidenceCellStatus::Unavailable);
    }

    #[test]
    fn should_keep_explicitly_skipped_cells_distinct_from_missing_artifacts() {
        // Arrange
        let arguments = [
            "--expected-source-commit",
            "0123456789abcdef0123456789abcdef01234567",
            "--expected-run-id",
            "selected-shard-test",
            "--skipped-cell",
            "tier2_subsystem_binder/linux-arm64",
        ]
        .into_iter()
        .map(std::ffi::OsString::from);

        // Act
        let plan = parse_plan(arguments).expect("explicit skip should be recorded");

        // Assert
        assert!(matches!(
            plan.inputs.as_slice(),
            [ValidationInput::Skipped(cell)] if cell == "tier2_subsystem_binder/linux-arm64"
        ));
        assert_eq!(
            plan.expected_source_commit,
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(plan.expected_run_id, "selected-shard-test");
        assert_eq!(EvidenceCellStatus::Skipped.as_str(), "SKIPPED");
        assert_eq!(EvidenceCellStatus::Unavailable.as_str(), "UNAVAILABLE");
    }

    #[test]
    fn should_not_pass_when_every_cell_is_explicitly_skipped() {
        // Arrange
        let arguments = [
            "--expected-source-commit",
            "0123456789abcdef0123456789abcdef01234567",
            "--expected-run-id",
            "selected-shard-test",
            "--skipped-cell",
            "tier2_subsystem_column_scan/linux-arm64",
        ]
        .into_iter()
        .map(std::ffi::OsString::from);
        let plan = parse_plan(arguments).expect("explicit skip should be recorded");

        // Act
        let has_artifact = has_selected_artifact(&plan.inputs);

        // Assert
        assert!(!has_artifact);
    }

    #[test]
    fn should_accept_canonical_release_artifact_with_complete_provenance() {
        // Arrange
        let file = TestArtifactFile::new();
        let run = current_stress_run(false);
        std::fs::write(
            &file.0,
            serde_json::to_vec(&run).expect("run should serialize"),
        )
        .expect("test artifact should be written");

        // Act
        let report = inspect_artifact(
            &file.0,
            "0123456789abcdef0123456789abcdef01234567",
            "selected-shard-test",
        )
        .expect("JSON artifact should load");

        // Assert
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(
            report.source_commit,
            "0123456789abcdef0123456789abcdef01234567"
        );
        assert_eq!(report.run_id, "selected-shard-test");
        assert_eq!(report.deployment_profile, "native-linux-arm64-disk");
        assert_eq!(report.rows.len(), 8);
        assert!(report
            .rows
            .iter()
            .all(|row| row.status == EvidenceCellStatus::GateCandidate));
    }

    #[test]
    fn should_reject_a_shard_missing_one_required_column_scenario() {
        // Arrange
        let file = TestArtifactFile::new();
        let mut run = current_stress_run(false);
        let missing_id = run
            .summaries
            .iter()
            .find(|summary| {
                summary
                    .metadata
                    .get("scenario_id")
                    .is_some_and(|id| id == super::COLUMN_SCENARIOS[0])
            })
            .expect("candidate summary should exist")
            .benchmark_id
            .clone();
        run.benchmark_specs.retain(|spec| spec.id != missing_id);
        run.summaries
            .retain(|summary| summary.benchmark_id != missing_id);
        run.samples
            .retain(|sample| sample.benchmark_id != missing_id);
        std::fs::write(
            &file.0,
            serde_json::to_vec(&run).expect("run should serialize"),
        )
        .expect("test artifact should be written");

        // Act
        let report = inspect_artifact(
            &file.0,
            "0123456789abcdef0123456789abcdef01234567",
            "selected-shard-test",
        )
        .expect("JSON artifact should load");

        // Assert
        assert!(report.errors.iter().any(|error| {
            error.contains("perf.column.selective_encoded_scan.2k")
                && error.contains("expected one")
        }));
    }

    #[test]
    fn should_reject_a_column_scenario_that_fails_its_relative_p95_gate() {
        // Arrange
        let file = TestArtifactFile::new();
        let run = current_stress_run(true);
        std::fs::write(
            &file.0,
            serde_json::to_vec(&run).expect("run should serialize"),
        )
        .expect("test artifact should be written");

        // Act
        let report = inspect_artifact(
            &file.0,
            "0123456789abcdef0123456789abcdef01234567",
            "selected-shard-test",
        )
        .expect("JSON artifact should load");

        // Assert
        assert!(report.errors.iter().any(|error| {
            error.contains("relative p95 gate failed")
                && error.contains("perf.column.selective_encoded_scan.2k")
        }));
    }

    #[test]
    fn should_reject_unknown_cpu_identity_in_a_saved_shard_artifact() {
        // Arrange
        let file = TestArtifactFile::new();
        let mut run = current_stress_run(false);
        run.environment.cpu_model = "unknown".to_string();
        for sample in &mut run.samples {
            sample.environment.cpu_model = "unknown".to_string();
        }
        std::fs::write(
            &file.0,
            serde_json::to_vec(&run).expect("run should serialize"),
        )
        .expect("test artifact should be written");

        // Act
        let report = inspect_artifact(
            &file.0,
            "0123456789abcdef0123456789abcdef01234567",
            "selected-shard-test",
        )
        .expect("JSON artifact should load");

        // Assert
        assert!(report
            .errors
            .iter()
            .any(|error| error.to_ascii_lowercase().contains("cpu model")));
    }

    #[test]
    fn should_reject_artifacts_with_unexpected_source_or_run_identity() {
        // Arrange
        let file = TestArtifactFile::new();
        let run = current_stress_run(false);
        std::fs::write(
            &file.0,
            serde_json::to_vec(&run).expect("run should serialize"),
        )
        .expect("test artifact should be written");

        // Act
        let report = inspect_artifact(
            &file.0,
            "fedcba9876543210fedcba9876543210fedcba98",
            "another-run",
        )
        .expect("JSON artifact should load");

        // Assert
        assert!(report
            .errors
            .iter()
            .any(|error| error.contains("does not match expected")));
        assert_eq!(report.errors.len(), 2);
    }

    #[test]
    fn should_reject_architecture_that_disagrees_with_native_profile() {
        // Arrange
        let file = TestArtifactFile::new();
        let mut run = current_stress_run(false);
        run.environment.os = "linux x86_64".to_string();
        for sample in &mut run.samples {
            sample.environment.os = "linux x86_64".to_string();
        }
        std::fs::write(
            &file.0,
            serde_json::to_vec(&run).expect("run should serialize"),
        )
        .expect("test artifact should be written");

        // Act
        let report = inspect_artifact(
            &file.0,
            "0123456789abcdef0123456789abcdef01234567",
            "selected-shard-test",
        )
        .expect("JSON artifact should load");

        // Assert
        assert!(report
            .errors
            .iter()
            .any(|error| { error.contains("does not match deployment profile") }));
    }

    #[test]
    fn should_reject_summaries_that_disagree_with_raw_samples() {
        // Arrange
        let file = TestArtifactFile::new();
        let mut run = current_stress_run(false);
        run.summaries
            .first_mut()
            .expect("run has one summary")
            .measured_samples += 1;
        std::fs::write(
            &file.0,
            serde_json::to_vec(&run).expect("run should serialize"),
        )
        .expect("test artifact should be written");

        // Act
        let report = inspect_artifact(
            &file.0,
            "0123456789abcdef0123456789abcdef01234567",
            "selected-shard-test",
        )
        .expect("JSON artifact should load");

        // Assert
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.to_ascii_lowercase().contains("summary")),
            "{:?}",
            report.errors
        );
    }
}
