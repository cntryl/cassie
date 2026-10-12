use std::collections::BTreeSet;
use std::io;
use std::sync::Mutex;

#[cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "arm"),
    not(test)
))]
use cntryl_stress::StressRunner;
use cntryl_stress::StressRunnerConfig;
use cntryl_stress::{
    artifact::{BenchmarkSpec, BenchmarkSummary, StressRun},
    reporting::{ConsoleReporter, JsonReporter, Reporter, SampleProgress, StderrProgressReporter},
};
#[cfg(any(
    all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")),
    test
))]
use std::io::Read;

const MAX_CPUINFO_BYTES: usize = 1_048_576;
const MAX_CPUINFO_PROCESSORS: usize = 4_096;
const MAX_CPU_IDENTITY_BYTES: usize = 512;
const MAX_DISTINCT_CPU_PARTS: usize = 16;

#[derive(Default)]
struct CpuInfoRecord {
    processor: Option<usize>,
    implementer: Option<u32>,
    part: Option<u32>,
}

pub(super) fn parse_linux_arm_cpu_identity(cpuinfo: &str) -> Option<String> {
    if cpuinfo.len() > MAX_CPUINFO_BYTES {
        return None;
    }

    let mut record = CpuInfoRecord::default();
    let mut identities = BTreeSet::new();
    let mut processor_ids = BTreeSet::new();

    for line in cpuinfo.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            finish_record(&mut record, &mut identities, &mut processor_ids)?;
            continue;
        }

        let Some((key, value)) = trimmed.split_once(':') else {
            if is_cpu_identity_field(trimmed) {
                return None;
            }
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        if key.eq_ignore_ascii_case("processor") {
            if record.processor.is_some() {
                finish_record(&mut record, &mut identities, &mut processor_ids)?;
            }
            record.processor = Some(value.parse::<usize>().ok()?);
        } else if key.eq_ignore_ascii_case("cpu implementer") {
            if record.implementer.is_some() {
                return None;
            }
            record.implementer = Some(parse_hex_identity(value)?);
        } else if key.eq_ignore_ascii_case("cpu part") {
            if record.part.is_some() {
                return None;
            }
            record.part = Some(parse_hex_identity(value)?);
        }
    }

    finish_record(&mut record, &mut identities, &mut processor_ids)?;
    if identities.is_empty() || identities.len() > MAX_DISTINCT_CPU_PARTS {
        return None;
    }

    let identity = identities
        .into_iter()
        .map(|(implementer, part)| format!("ARM CPU implementer=0x{implementer:x} part=0x{part:x}"))
        .collect::<Vec<_>>()
        .join("; ");

    (identity.len() <= MAX_CPU_IDENTITY_BYTES).then_some(identity)
}

fn finish_record(
    record: &mut CpuInfoRecord,
    identities: &mut BTreeSet<(u32, u32)>,
    processor_ids: &mut BTreeSet<usize>,
) -> Option<()> {
    if record.processor.is_some() || record.implementer.is_some() || record.part.is_some() {
        let processor_id = record.processor?;
        if !processor_ids.insert(processor_id) || processor_ids.len() > MAX_CPUINFO_PROCESSORS {
            return None;
        }
        let identity = record.implementer.zip(record.part)?;
        identities.insert(identity);
    }

    *record = CpuInfoRecord::default();
    Some(())
}

fn is_cpu_identity_field(line: &str) -> bool {
    ["processor", "cpu implementer", "cpu part"]
        .iter()
        .any(|field| {
            line.get(..field.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(field))
        })
}

fn parse_hex_identity(value: &str) -> Option<u32> {
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))?;
    if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    u32::from_str_radix(digits, 16).ok()
}

pub(super) fn apply_captured_cpu_identity(
    run: &mut StressRun,
    captured_identity: Option<&str>,
) -> io::Result<()> {
    let run_identity = run.environment.cpu_model.as_str();
    let replacement = if run_identity.eq_ignore_ascii_case("unknown") {
        captured_identity.map(validate_cpu_identity).transpose()?
    } else {
        None
    };

    let run_identity_is_unknown = run_identity.eq_ignore_ascii_case("unknown");
    let samples_match = run.samples.iter().all(|sample| match replacement {
        Some(identity) => {
            sample.environment.cpu_model.eq_ignore_ascii_case("unknown")
                || sample.environment.cpu_model == identity
        }
        None if run_identity_is_unknown => {
            sample.environment.cpu_model.eq_ignore_ascii_case("unknown")
        }
        None => sample.environment.cpu_model == run_identity,
    });
    if !samples_match {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "benchmark run and sample CPU identities disagree",
        ));
    }

    if let Some(replacement) = replacement {
        run.environment.cpu_model = replacement.to_string();
        for sample in &mut run.samples {
            sample.environment.cpu_model = replacement.to_string();
        }
    }

    Ok(())
}

#[cfg(any(
    all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")),
    test
))]
pub(super) fn read_bounded_cpuinfo(reader: impl Read) -> io::Result<Vec<u8>> {
    let limit = u64::try_from(MAX_CPUINFO_BYTES)
        .map_err(io::Error::other)?
        .saturating_add(1);
    let mut bytes = Vec::new();
    reader.take(limit).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_CPUINFO_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Linux CPU info exceeds the collection bound",
        ));
    }
    Ok(bytes)
}

fn validate_cpu_identity(identity: &str) -> io::Result<&str> {
    if identity.is_empty()
        || identity.len() > MAX_CPU_IDENTITY_BYTES
        || identity.trim() != identity
        || identity.eq_ignore_ascii_case("unknown")
        || identity.chars().any(char::is_control)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "captured benchmark CPU identity is empty, unknown, or malformed",
        ));
    }
    Ok(identity)
}

#[cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "arm"),
    not(test)
))]
fn capture_linux_arm_cpu_identity() -> Option<String> {
    let file = std::fs::File::open("/proc/cpuinfo").ok()?;
    let bytes = read_bounded_cpuinfo(file).ok()?;
    let cpuinfo = String::from_utf8(bytes).ok()?;
    parse_linux_arm_cpu_identity(&cpuinfo)
}

/// Publishes the current benchmark run with a bounded ARM identity attached.
///
/// The stress crate's captured environment is private. This reporter uses its
/// supported reporter interface to publish a clone of the live run, attaching
/// one CPU identity captured before measurement to the run and every sample.
pub(super) struct ArmCpuIdentityReporter {
    captured_identity: Option<String>,
    artifacts: JsonReporter,
    json_stdout: bool,
    console: ConsoleReporter,
    progress: Option<StderrProgressReporter>,
    stdout_lock: Mutex<()>,
}

impl ArmCpuIdentityReporter {
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "aarch64", target_arch = "arm"),
        not(test)
    ))]
    fn new(config: &StressRunnerConfig) -> Self {
        Self::build(config, capture_linux_arm_cpu_identity())
    }

    #[cfg(test)]
    pub(super) fn with_identity(
        config: &StressRunnerConfig,
        captured_identity: Option<String>,
    ) -> Self {
        Self::build(config, captured_identity)
    }

    #[cfg(any(
        test,
        all(
            target_os = "linux",
            any(target_arch = "aarch64", target_arch = "arm"),
            not(test)
        )
    ))]
    fn build(config: &StressRunnerConfig, captured_identity: Option<String>) -> Self {
        Self {
            captured_identity,
            artifacts: JsonReporter::new(config.output_dir.clone()).announce(false),
            json_stdout: config.json_stdout,
            console: ConsoleReporter::new(),
            progress: (!config.json_stdout && config.progress).then(StderrProgressReporter::new),
            stdout_lock: Mutex::new(()),
        }
    }

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "aarch64", target_arch = "arm"),
        not(test)
    ))]
    pub(super) fn install(runner: &mut StressRunner, config: &StressRunnerConfig) {
        runner.reporters(vec![Box::new(Self::new(config))]);
    }
}

impl Reporter for ArmCpuIdentityReporter {
    fn bench_start(&self, spec: &BenchmarkSpec) {
        if let Some(progress) = &self.progress {
            progress.bench_start(spec);
        }
    }

    fn sample_progress(&self, progress: &SampleProgress) {
        if let Some(reporter) = &self.progress {
            reporter.sample_progress(progress);
        }
    }

    fn bench_end(&self, summary: &BenchmarkSummary) {
        if let Some(progress) = &self.progress {
            progress.bench_end(summary);
        }
    }

    fn suite_end(&self, run: &StressRun) -> io::Result<()> {
        let mut published_run = run.clone();
        let publication_result =
            apply_captured_cpu_identity(&mut published_run, self.captured_identity.as_deref())
                .and_then(|()| self.artifacts.suite_end(&published_run));
        if let Err(error) = &publication_result {
            published_run
                .metadata
                .entry("reporter_errors".to_string())
                .and_modify(|existing| {
                    existing.push_str("; ");
                    existing.push_str(&error.to_string());
                })
                .or_insert_with(|| error.to_string());
        }

        if self.json_stdout {
            use std::io::Write as _;

            let _guard = self
                .stdout_lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut stdout = io::stdout().lock();
            serde_json::to_writer_pretty(&mut stdout, &published_run).map_err(io::Error::other)?;
            writeln!(stdout)?;
        } else {
            self.console.suite_end(&published_run)?;
        }
        publication_result
    }
}
