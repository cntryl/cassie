use std::path::PathBuf;
use std::str::FromStr;

use cntryl_stress::{artifact::RunProfile, StressRunnerConfig};

use crate::performance_benchmarks::{self, BenchmarkTier};

use super::{
    resolve_soak_duration, soak_sample_duration, validate_soak_duration_for_profile, SoakDuration,
};

pub(super) struct ResolvedConfig {
    pub(super) config: StressRunnerConfig,
    pub(super) filter: Option<String>,
    pub(super) tier_filter: Option<u32>,
    pub(super) baseline: Option<PathBuf>,
    pub(super) print_config: bool,
    pub(super) soak_duration: Option<SoakDuration>,
}

struct CommandLineConfig {
    config: StressRunnerConfig,
    filter: Option<String>,
    tier_filter: Option<u32>,
    baseline: Option<PathBuf>,
    print_config: bool,
    soak_duration: Option<String>,
}

pub(super) fn resolve_config(tier: BenchmarkTier) -> ResolvedConfig {
    let mut config = StressRunnerConfig::from_env();
    let filter = config.filter.take();
    let tier_filter = config.tier.take();
    let command_line = parse_command_line(CommandLineConfig {
        config,
        filter,
        tier_filter,
        baseline: std::env::var_os("STRESS_BASELINE").map(PathBuf::from),
        print_config: false,
        soak_duration: None,
    });
    let CommandLineConfig {
        mut config,
        filter,
        tier_filter,
        baseline,
        print_config,
        soak_duration,
    } = command_line;

    config.fail_on_quality = false;
    config.deny_diagnostics = None;
    let soak_duration = if tier == BenchmarkTier::Tier6 {
        let environment = std::env::var("CASSIE_BENCH_SOAK_DURATION_SECONDS").ok();
        let resolved = resolve_soak_duration(environment.as_deref(), soak_duration.as_deref())
            .unwrap_or_else(|error| panic!("invalid Tier 6 duration: {error}"));
        validate_soak_duration_for_profile(resolved.total, config.profile == RunProfile::Smoke)
            .unwrap_or_else(|error| panic!("invalid Tier 6 duration: {error}"));
        config.sample_duration = soak_sample_duration(resolved.total, config.samples)
            .unwrap_or_else(|error| panic!("invalid Tier 6 sample duration: {error}"));
        config.warmup_samples = 0;
        config.cooldown_samples = 0;
        Some(resolved)
    } else {
        None
    };
    let diagnostic_run =
        filter.is_some() || tier_filter.is_some() || config.profile == RunProfile::Smoke;
    route_filtered_artifacts(&mut config, diagnostic_run);

    ResolvedConfig {
        config,
        filter,
        tier_filter,
        baseline,
        print_config,
        soak_duration,
    }
}

fn parse_command_line(mut parsed: CommandLineConfig) -> CommandLineConfig {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut index = 0;
    let mut bare_filters = Vec::new();

    while index < args.len() {
        let arg = &args[index];
        let (name, inline_value) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(name, value)| (name, Some(value)));
        let value = if inline_value.is_some() || !option_takes_value(name) {
            inline_value
        } else {
            index += 1;
            args.get(index).map(String::as_str)
        };
        match (name, value) {
            ("--workload", Some(value)) => parsed.filter = Some(value.to_string()),
            ("--tier", Some(value)) => parsed.tier_filter = parse_u32(value),
            ("--profile", Some(value)) => apply_profile(&mut parsed.config, value),
            ("--samples", Some(value)) => apply_samples(&mut parsed.config, value),
            ("--warmup-samples", Some(value)) => apply_warmup(&mut parsed.config, value),
            ("--console", Some(value)) => apply_console(&mut parsed.config, value),
            ("--output-dir", Some(value)) => {
                parsed.config = parsed.config.clone().output_dir(value);
            }
            ("--soak-duration-seconds", Some(value)) => {
                parsed.soak_duration = Some(value.to_string());
            }
            ("--baseline", Some(value)) => parsed.baseline = Some(PathBuf::from(value)),
            ("--json", _) => parsed.config = parsed.config.clone().json_stdout(true),
            ("--print-config", _) => parsed.print_config = true,
            _ => {}
        }
        if !arg.starts_with("--") {
            bare_filters.push(arg.clone());
        }
        index += 1;
    }

    if parsed.filter.is_none() && !bare_filters.is_empty() {
        parsed.filter = Some(bare_filters.join(" "));
    }
    parsed
}

fn option_takes_value(option: &str) -> bool {
    matches!(
        option,
        "--workload"
            | "--tier"
            | "--profile"
            | "--samples"
            | "--warmup-samples"
            | "--console"
            | "--output-dir"
            | "--soak-duration-seconds"
            | "--baseline"
    )
}

fn apply_profile(config: &mut StressRunnerConfig, value: &str) {
    if let Ok(profile) = RunProfile::from_str(value) {
        *config = config.clone().profile(profile);
    }
}

fn apply_samples(config: &mut StressRunnerConfig, value: &str) {
    if let Ok(samples) = value.parse() {
        *config = config.clone().samples(samples);
    }
}

fn apply_warmup(config: &mut StressRunnerConfig, value: &str) {
    if let Ok(samples) = value.parse() {
        *config = config.clone().warmup_samples(samples);
    }
}

fn apply_console(config: &mut StressRunnerConfig, value: &str) {
    let json_stdout = match value {
        "json" => Some(true),
        "compact" | "full" | "verbose" | "ci" => Some(false),
        _ => None,
    };
    if let Some(json_stdout) = json_stdout {
        *config = config.clone().json_stdout(json_stdout);
    }
}

fn route_filtered_artifacts(config: &mut StressRunnerConfig, filtered_run: bool) {
    config.output_dir =
        performance_benchmarks::artifact_output_dir(&config.output_dir, filtered_run);
}

fn parse_u32(value: &str) -> Option<u32> {
    value.parse().ok()
}

pub(super) fn print_config(suite: &str, config: &StressRunnerConfig, filter: Option<&str>) {
    println!("Benchmark Suite: {suite}");
    println!("Profile: {}", config.profile);
    println!("Samples: {}", config.samples);
    println!("Warmup samples: {}", config.warmup_samples);
    println!("Cooldown samples: {}", config.cooldown_samples);
    println!("Output: {}", config.output_dir.display());
    println!("JSON stdout: {}", config.json_stdout);
    println!("Filter: {}", filter.unwrap_or("<none>"));
    println!(
        "Tier: {}",
        config
            .tier
            .map_or_else(|| "<any>".to_string(), |tier| tier.to_string())
    );
}

pub(super) fn matches_filter(value: &str, filter: &str) -> bool {
    filter
        .split_whitespace()
        .any(|pattern| matches_pattern(value, pattern))
}

fn matches_pattern(value: &str, pattern: &str) -> bool {
    if pattern == "*" || value.contains(pattern) {
        return true;
    }

    let mut remaining = value;
    let anchored_start = !pattern.starts_with('*');
    let anchored_end = !pattern.ends_with('*');
    let parts = pattern
        .split('*')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if parts.is_empty() {
        return true;
    }
    if anchored_start && !value.starts_with(parts[0]) {
        return false;
    }

    for part in &parts {
        let Some(position) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[position + part.len()..];
    }

    !anchored_end || value.ends_with(parts[parts.len() - 1])
}
