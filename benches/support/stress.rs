#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{Duration, Instant};

use cntryl_stress::{
    black_box, evaluate_run_gate, BenchmarkBudgets, BenchmarkModeKind, BenchmarkSpec,
    MeasurementIntent, RunGate, RunProfile, StressContext, StressRunner, StressRunnerConfig,
};

use crate::performance_benchmarks;

pub struct CassieStressRunner {
    suite: &'static str,
    config: StressRunnerConfig,
    filter: Option<String>,
    tier_filter: Option<u32>,
    baseline: Option<PathBuf>,
    runner: StressRunner,
    selected: usize,
}

#[derive(Debug, Clone)]
pub struct StressCase {
    tier: u32,
    mode: BenchmarkModeKind,
    workload: String,
    fixture_scale: String,
    parameters: BTreeMap<String, String>,
    metadata: BTreeMap<String, String>,
    intent: MeasurementIntent,
}

impl StressCase {
    pub fn tier1_micro(workload: impl Into<String>) -> Self {
        Self {
            tier: 1,
            mode: BenchmarkModeKind::Micro,
            workload: workload.into(),
            fixture_scale: "micro".to_string(),
            parameters: BTreeMap::new(),
            metadata: BTreeMap::new(),
            intent: MeasurementIntent::General,
        }
    }

    pub fn fixed_operations(
        tier: u32,
        workload: impl Into<String>,
        fixture_scale: impl Into<String>,
    ) -> Self {
        Self {
            tier,
            mode: BenchmarkModeKind::FixedOperations,
            workload: workload.into(),
            fixture_scale: fixture_scale.into(),
            parameters: BTreeMap::new(),
            metadata: BTreeMap::new(),
            intent: MeasurementIntent::General,
        }
    }

    pub fn fixed_duration(
        tier: u32,
        workload: impl Into<String>,
        fixture_scale: impl Into<String>,
    ) -> Self {
        Self {
            tier,
            mode: BenchmarkModeKind::FixedDuration,
            workload: workload.into(),
            fixture_scale: fixture_scale.into(),
            parameters: BTreeMap::new(),
            metadata: BTreeMap::new(),
            intent: MeasurementIntent::General,
        }
    }

    pub fn parameter(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.parameters.insert(key.into(), value.into());
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    fn intent(mut self, intent: MeasurementIntent) -> Self {
        self.intent = intent;
        self
    }

    fn measurement_name(&self) -> String {
        format!("{}/{}", self.workload, self.fixture_scale)
    }
}

pub fn runner(suite: &'static str) -> CassieStressRunner {
    CassieStressRunner::new(suite)
}

impl CassieStressRunner {
    pub fn new(suite: &'static str) -> Self {
        let resolved = resolve_config();
        if resolved.print_config {
            print_config(suite, &resolved.config, resolved.filter.as_deref());
            std::process::exit(0);
        }

        let runner = StressRunner::with_config(suite, resolved.config.clone());
        Self {
            suite,
            config: resolved.config,
            filter: resolved.filter,
            tier_filter: resolved.tier_filter,
            baseline: resolved.baseline,
            runner,
            selected: 0,
        }
    }

    pub fn tier1_micro<F, R>(&mut self, workload: &'static str, f: F)
    where
        F: FnMut() -> R,
    {
        self.micro(StressCase::tier1_micro(workload), f);
    }

    pub fn micro<F, R>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> R,
    {
        let f = RefCell::new(f);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            black_box(ctx.measure(&measurement_name, || (f.borrow_mut())()));
        });
    }

    pub fn fixed_operations<F, R>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> R,
    {
        let f = RefCell::new(f);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            ctx.measure(&measurement_name, || {
                black_box((f.borrow_mut())());
            });
        });
    }

    pub fn fixed_counted<F>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> u64,
    {
        let f = RefCell::new(f);
        let case = case.intent(MeasurementIntent::External);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let started = Instant::now();
            let completed = (f.borrow_mut())();
            ctx.record_external(&measurement_name, started.elapsed(), completed);
        });
    }

    pub fn fixed_counted_usize<F>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> usize,
    {
        let f = RefCell::new(f);
        self.fixed_counted(case, || {
            u64::try_from((f.borrow_mut())()).expect("benchmark count should fit u64")
        });
    }

    pub fn fixed_batch<F, R>(&mut self, case: StressCase, logical_operations: u64, f: F)
    where
        F: FnMut() -> R,
    {
        let f = RefCell::new(f);
        let case = case
            .intent(MeasurementIntent::Batch)
            .parameter(
                "logical_operations_per_iteration",
                logical_operations.to_string(),
            )
            .metadata(
                "logical_operations_per_iteration",
                logical_operations.to_string(),
            );
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let _completed = ctx.measure_batch(&measurement_name, logical_operations, || {
                black_box((f.borrow_mut())());
            });
        });
    }

    pub fn fixed_timed_count<F, R>(&mut self, case: StressCase, logical_operations: u64, f: F)
    where
        F: FnMut() -> R,
    {
        let f = RefCell::new(f);
        let case = case
            .intent(MeasurementIntent::External)
            .parameter(
                "logical_operations_per_iteration",
                logical_operations.to_string(),
            )
            .metadata(
                "logical_operations_per_iteration",
                logical_operations.to_string(),
            );
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let started = Instant::now();
            black_box((f.borrow_mut())());
            let elapsed = started.elapsed();
            ctx.record_external(&measurement_name, elapsed, logical_operations);
        });
    }

    pub fn fixed_timed_counted_usize<F>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> usize,
    {
        let f = RefCell::new(f);
        let case = case.intent(MeasurementIntent::External);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let started = Instant::now();
            let completed =
                u64::try_from((f.borrow_mut())()).expect("benchmark count should fit u64");
            let elapsed = started.elapsed();
            ctx.record_external(&measurement_name, elapsed, completed);
        });
    }

    pub fn fixed_single<F, R>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> R,
    {
        let f = RefCell::new(f);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            black_box(ctx.measure(&measurement_name, || (f.borrow_mut())()));
        });
    }

    pub fn external_timed_batch<F>(&mut self, case: StressCase, operation_count: u64, f: F)
    where
        F: FnMut() -> Duration,
    {
        let f = RefCell::new(f);
        let case = case
            .intent(MeasurementIntent::External)
            .parameter("operation_count", operation_count.to_string());
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            let duration = (f.borrow_mut())();
            let multiplier =
                u32::try_from(operation_count).expect("batch operation count fits Duration");
            ctx.record_external(&measurement_name, duration * multiplier, operation_count);
        });
    }

    pub fn fixed_duration<F, R>(&mut self, case: StressCase, f: F)
    where
        F: FnMut() -> R,
    {
        let f = RefCell::new(f);
        let measurement_name = case.measurement_name();
        self.run_case(case, move |ctx| {
            ctx.measure(&measurement_name, || {
                black_box((f.borrow_mut())());
            });
        });
    }

    pub fn is_enabled(&self, case: &StressCase) -> bool {
        self.should_run(case)
    }

    pub fn finish(self) {
        if self.selected == 0 {
            eprintln!("No stress benchmarks matched the selected filters.");
        }

        let run = if let Some(baseline) = self.baseline {
            self.runner.finish_with_baseline(baseline)
        } else {
            Ok(self.runner.finish())
        };

        match run {
            Ok(run) => {
                let gate = evaluate_run_gate(&run);
                if gate != RunGate::Passed {
                    eprintln!("Stress run failed: {gate:?}");
                    std::process::exit(1);
                }
            }
            Err(error) => {
                eprintln!("Stress run failed to load baseline: {error}");
                std::process::exit(1);
            }
        }
    }

    fn run_case<F>(&mut self, case: StressCase, f: F)
    where
        F: Fn(&mut StressContext),
    {
        if !self.should_run(&case) {
            return;
        }

        self.selected = self.selected.saturating_add(1);
        let spec = self.spec_for(case);
        self.runner.run_spec(&spec, f);
    }

    fn should_run(&self, case: &StressCase) -> bool {
        if self.tier_filter.is_some_and(|tier| tier != case.tier) {
            return false;
        }

        self.filter.as_deref().is_none_or(|filter| {
            let scenario = performance_benchmarks::benchmark_for_benchmark(
                self.suite,
                &case.workload,
                &case.fixture_scale,
            );
            let mut candidates = vec![
                self.suite.to_string(),
                case.workload.clone(),
                case.fixture_scale.clone(),
                format!("{}/{}", case.workload, case.fixture_scale),
                format!("{}/{}/{}", self.suite, case.workload, case.fixture_scale),
            ];
            if let Some(scenario) = scenario {
                candidates.push(scenario.scenario_id.to_string());
                candidates.push(scenario.family.to_string());
            }
            candidates
                .iter()
                .any(|candidate| matches_filter(candidate, filter))
        })
    }

    fn spec_for(&self, case: StressCase) -> BenchmarkSpec {
        let scenario = performance_benchmarks::benchmark_for_benchmark(
            self.suite,
            &case.workload,
            &case.fixture_scale,
        );
        let mut metadata = BTreeMap::from([
            ("benchmark".to_string(), self.suite.to_string()),
            ("workload".to_string(), case.workload.clone()),
            ("fixture_scale".to_string(), case.fixture_scale.clone()),
        ]);
        if let Some(scenario) = scenario {
            metadata.insert("scenario_id".to_string(), scenario.scenario_id.to_string());
            metadata.insert("family".to_string(), scenario.family.to_string());
        }
        metadata.extend(case.metadata);

        let id = format!("{}/{}/{}", self.suite, case.workload, case.fixture_scale);
        BenchmarkSpec {
            id,
            name: format!("{}/{}", case.workload, case.fixture_scale),
            tier: case.tier,
            mode: self
                .config
                .mode_for_tier(case.tier)
                .unwrap_or_else(|| self.config.mode_for_kind(case.mode)),
            intent: case.intent,
            budgets: BenchmarkBudgets::default(),
            parameters: case.parameters,
            metadata,
        }
    }
}

struct ResolvedConfig {
    config: StressRunnerConfig,
    filter: Option<String>,
    tier_filter: Option<u32>,
    baseline: Option<PathBuf>,
    print_config: bool,
}

fn resolve_config() -> ResolvedConfig {
    let mut config = StressRunnerConfig::from_env();
    let mut filter = config.filter.take();
    let mut tier_filter = config.tier.take();
    let mut baseline = std::env::var_os("STRESS_BASELINE").map(PathBuf::from);
    let mut print_config = false;
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let mut index = 0;
    let mut bare_filters = Vec::new();

    while index < args.len() {
        let arg = &args[index];
        if let Some(value) = arg.strip_prefix("--workload=") {
            filter = Some(value.to_string());
        } else if arg == "--workload" {
            if let Some(value) = args.get(index + 1) {
                filter = Some(value.clone());
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--tier=") {
            tier_filter = parse_u32(value);
        } else if arg == "--tier" {
            if let Some(value) = args.get(index + 1) {
                tier_filter = parse_u32(value);
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--profile=") {
            if let Some(profile) = parse_profile(value) {
                config = config.profile(profile);
            }
        } else if arg == "--profile" {
            if let Some(value) = args.get(index + 1) {
                if let Some(profile) = parse_profile(value) {
                    config = config.profile(profile);
                }
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--samples=") {
            if let Some(samples) = parse_usize(value) {
                config = config.samples(samples);
            }
        } else if arg == "--samples" {
            if let Some(value) = args.get(index + 1).and_then(|value| parse_usize(value)) {
                config = config.samples(value);
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--warmup-samples=") {
            if let Some(samples) = parse_usize(value) {
                config = config.warmup_samples(samples);
            }
        } else if arg == "--warmup-samples" {
            if let Some(value) = args.get(index + 1).and_then(|value| parse_usize(value)) {
                config = config.warmup_samples(value);
                index += 1;
            }
        } else if arg == "--json" {
            config = config.json_stdout(true);
        } else if let Some(value) = arg.strip_prefix("--console=") {
            if let Some(json_stdout) = parse_console_json_stdout(value) {
                config = config.json_stdout(json_stdout);
            }
        } else if arg == "--console" {
            if let Some(value) = args.get(index + 1) {
                if let Some(json_stdout) = parse_console_json_stdout(value) {
                    config = config.json_stdout(json_stdout);
                }
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--output-dir=") {
            config = config.output_dir(value);
        } else if arg == "--output-dir" {
            if let Some(value) = args.get(index + 1) {
                config = config.output_dir(value);
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--baseline=") {
            baseline = Some(PathBuf::from(value));
        } else if arg == "--baseline" {
            if let Some(value) = args.get(index + 1) {
                baseline = Some(PathBuf::from(value));
                index += 1;
            }
        } else if arg == "--print-config" {
            print_config = true;
        } else if !arg.starts_with("--") {
            bare_filters.push(arg.clone());
        }
        index += 1;
    }

    if filter.is_none() && !bare_filters.is_empty() {
        filter = Some(bare_filters.join(" "));
    }

    ResolvedConfig {
        config,
        filter,
        tier_filter,
        baseline,
        print_config,
    }
}

fn parse_profile(value: &str) -> Option<RunProfile> {
    RunProfile::from_str(value).ok()
}

fn parse_console_json_stdout(value: &str) -> Option<bool> {
    match value {
        "json" => Some(true),
        "compact" | "full" | "verbose" | "ci" => Some(false),
        _ => None,
    }
}

fn parse_u32(value: &str) -> Option<u32> {
    value.parse().ok()
}

fn parse_usize(value: &str) -> Option<usize> {
    value.parse().ok()
}

fn print_config(suite: &str, config: &StressRunnerConfig, filter: Option<&str>) {
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

fn matches_filter(value: &str, filter: &str) -> bool {
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
