use std::path::Path;
use std::process::{Command, ExitCode};

use cassie_bench_support::performance_benchmarks::validate_complete_benchmark_suite;
use cassie_bench_support::registry::{registered_targets, validate_cargo_metadata};
use serde_json::json;

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(usage)?;
    match command.as_str() {
        "registry" => {
            let targets = registered_targets();
            println!(
                "{}",
                serde_json::to_string(&targets).map_err(|error| error.to_string())?
            );
        }
        "check" => {
            if args.next().is_some() {
                return Err(usage());
            }
            let output = Command::new("cargo")
                .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
                .output()
                .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
            if !output.status.success() {
                return Err(format!(
                    "cargo metadata failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            let metadata = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
            validate_cargo_metadata(&registered_targets(), &metadata)?;
            println!(
                "{}",
                json!({"ok": true, "registered_targets": registered_targets().len()})
            );
        }
        "validate" => {
            let path = args.next().ok_or_else(usage)?;
            if args.next().is_some() {
                return Err(usage());
            }
            validate_complete_benchmark_suite(Path::new(&path))?;
            println!("{}", json!({"ok": true}));
        }
        _ => return Err(usage()),
    }
    Ok(())
}

fn usage() -> String {
    "usage: cassie-bench registry | check | validate <stress-output-directory>".to_owned()
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", json!({"ok": false, "error": error}));
            ExitCode::FAILURE
        }
    }
}
