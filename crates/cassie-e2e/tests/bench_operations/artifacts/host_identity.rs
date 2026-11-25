#[path = "../../../benches/support/host_identity.rs"]
mod host_identity;

use cntryl_stress::artifact::{
    EnvironmentInfo, ProfileConfig, RunProfile, Sample, SamplePhase, StressRun,
};
use cntryl_stress::reporting::Reporter;
use cntryl_stress::StressRunnerConfig;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ARTIFACT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestArtifactDirectory(PathBuf);

impl TestArtifactDirectory {
    fn new() -> Self {
        let sequence = NEXT_ARTIFACT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        Self(std::env::temp_dir().join(format!(
            "cassie-host-identity-{}-{sequence}",
            std::process::id()
        )))
    }
}

impl Drop for TestArtifactDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const HOMOGENEOUS_ARM_CPUINFO: &str = "processor : 0\nCPU implementer : 0x41\nCPU architecture : 8\nCPU part : 0xd0b\nCPU revision : 0\n\nprocessor : 1\nCPU implementer : 0x41\nCPU architecture : 8\nCPU part : 0xd0b\nCPU revision : 0\n";

fn unknown_stress_run(sample_cpu_models: &[&str]) -> StressRun {
    let environment = EnvironmentInfo::unknown(ProfileConfig::new());
    let mut run = StressRun::new("cpu_identity", RunProfile::Smoke, environment.clone());
    run.samples = sample_cpu_models
        .iter()
        .enumerate()
        .map(|(sample_number, cpu_model)| {
            let mut sample =
                Sample::new("identity_probe", SamplePhase::Measured, environment.clone());
            sample.sample_number = sample_number;
            sample.environment.cpu_model = (*cpu_model).to_string();
            sample
        })
        .collect();
    run
}

#[test]
fn should_identify_arm_cpuinfo_without_model_name() {
    // Arrange
    let cpuinfo = HOMOGENEOUS_ARM_CPUINFO;

    // Act
    let identity = host_identity::parse_linux_arm_cpu_identity(cpuinfo);

    // Assert
    assert_eq!(
        identity.as_deref(),
        Some("ARM CPU implementer=0x41 part=0xd0b")
    );
}

#[test]
fn should_render_heterogeneous_arm_cpu_parts_in_stable_order() {
    // Arrange
    let cpuinfo = "processor : 0\nCPU implementer : 0x41\nCPU part : 0xd40\n\nprocessor : 1\nCPU implementer : 0x41\nCPU part : 0xd0b\n";

    // Act
    let identity = host_identity::parse_linux_arm_cpu_identity(cpuinfo);

    // Assert
    assert_eq!(
        identity.as_deref(),
        Some("ARM CPU implementer=0x41 part=0xd0b; ARM CPU implementer=0x41 part=0xd40")
    );
}

#[test]
fn should_reject_missing_malformed_or_incomplete_arm_cpu_identity() {
    // Arrange
    let cases = [
        "",
        "processor : 0\nCPU implementer : 0x41\n",
        "processor : 0\nCPU implementer : 0xZZ\nCPU part : 0xd0b\n",
        "processor : 0\nCPU implementer : 0x41\nCPU part : not-a-part\n",
        "processor : 0\nCPU implementer 0x41\nCPU part : 0xd0b\n",
    ];

    // Act
    let identities = cases
        .iter()
        .map(|cpuinfo| host_identity::parse_linux_arm_cpu_identity(cpuinfo))
        .collect::<Vec<_>>();

    // Assert
    assert!(identities.iter().all(Option::is_none));
}

#[test]
fn should_reject_duplicate_processor_ids_in_arm_cpuinfo() {
    // Arrange
    let cpuinfo = "processor : 0\nCPU implementer : 0x41\nCPU part : 0xd0b\n\nprocessor : 0\nCPU implementer : 0x41\nCPU part : 0xd40\n";

    // Act
    let identity = host_identity::parse_linux_arm_cpu_identity(cpuinfo);

    // Assert
    assert_eq!(identity, None);
}

#[test]
fn should_reject_arm_cpuinfo_larger_than_the_collection_bound() {
    // Arrange
    let cpuinfo = format!("{HOMOGENEOUS_ARM_CPUINFO}{}", "x".repeat(1_048_577));

    // Act
    let identity = host_identity::parse_linux_arm_cpu_identity(&cpuinfo);

    // Assert
    assert_eq!(identity, None);
}

#[test]
fn should_bound_cpuinfo_reads_before_parsing() {
    // Arrange
    let cpuinfo = vec![b'x'; 1_048_577];

    // Act
    let result = host_identity::read_bounded_cpuinfo(Cursor::new(cpuinfo));

    // Assert
    assert_eq!(
        result
            .expect_err("oversized CPU info must be rejected")
            .kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn should_apply_cpu_identity_consistently() {
    // Arrange
    let mut run = unknown_stress_run(&["unknown", "unknown"]);
    let identity = host_identity::parse_linux_arm_cpu_identity(HOMOGENEOUS_ARM_CPUINFO)
        .expect("fixture contains a truthful ARM CPU identity");

    // Act
    host_identity::apply_captured_cpu_identity(&mut run, Some(&identity))
        .expect("consistent host identity should be applied");

    // Assert
    assert_eq!(run.environment.cpu_model, identity);
    assert!(run
        .samples
        .iter()
        .all(|sample| sample.environment.cpu_model == identity));
}

#[test]
fn should_keep_unknown_cpu_identity_when_host_data_is_unavailable() {
    // Arrange
    let mut run = unknown_stress_run(&["unknown", "unknown"]);

    // Act
    host_identity::apply_captured_cpu_identity(&mut run, None)
        .expect("unavailable host identity remains an unknown fallback");

    // Assert
    assert_eq!(run.environment.cpu_model, "unknown");
    assert!(run
        .samples
        .iter()
        .all(|sample| sample.environment.cpu_model == "unknown"));
}

#[test]
fn should_keep_case_insensitive_unknown_fallback_consistent() {
    // Arrange
    let mut run = unknown_stress_run(&["Unknown", "unknown"]);
    run.environment.cpu_model = "UNKNOWN".to_string();

    // Act
    host_identity::apply_captured_cpu_identity(&mut run, None)
        .expect("case-insensitive unknown values remain the same fallback");

    // Assert
    assert_eq!(run.environment.cpu_model, "UNKNOWN");
    assert!(run
        .samples
        .iter()
        .all(|sample| sample.environment.cpu_model.eq_ignore_ascii_case("unknown")));
}

#[test]
fn should_reject_sample_identity_that_disagrees_with_the_run() {
    // Arrange
    let mut run = unknown_stress_run(&["unknown", "unrelated host"]);
    let original = run.clone();

    // Act
    let result = host_identity::apply_captured_cpu_identity(
        &mut run,
        Some("ARM CPU implementer=0x41 part=0xd0b"),
    );

    // Assert
    assert_eq!(
        result
            .expect_err("mismatched run and sample identity must be rejected")
            .kind(),
        std::io::ErrorKind::InvalidData
    );
    assert_eq!(run, original);
}

#[test]
fn should_publish_identity_consistently_in_saved_artifacts() {
    // Arrange
    let directory = TestArtifactDirectory::new();
    let identity = host_identity::parse_linux_arm_cpu_identity(HOMOGENEOUS_ARM_CPUINFO)
        .expect("fixture contains a truthful ARM CPU identity");
    let config = StressRunnerConfig::for_profile(RunProfile::Smoke)
        .output_dir(directory.0.clone())
        .json_stdout(false)
        .progress(false);
    let reporter =
        host_identity::ArmCpuIdentityReporter::with_identity(&config, Some(identity.clone()));
    let mut run = unknown_stress_run(&["unknown", "unknown"]);
    run.started_at = "host-identity-test".to_string();

    // Act
    reporter
        .suite_end(&run)
        .expect("the current benchmark run should be published");

    // Assert
    let artifact = directory.0.join("cpu_identity/latest.json");
    let published: StressRun = serde_json::from_slice(
        &std::fs::read(artifact).expect("published benchmark JSON should exist"),
    )
    .expect("published benchmark JSON should parse");
    assert_eq!(published.environment.cpu_model, identity);
    assert!(published
        .samples
        .iter()
        .all(|sample| sample.environment.cpu_model == identity));
    assert!(run.environment.cpu_model.eq_ignore_ascii_case("unknown"));
}
