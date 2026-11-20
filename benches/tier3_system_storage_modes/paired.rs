//! Paired heap-versus-column harness (`BENCH_TIER3_STORAGE_MODES_MODE=paired`).
//!
//! One process and one fixture serve both arms with the same query budget,
//! cache setting and worker count. Everything before timing is non-timing
//! evidence; a gate loss is recorded and printed, and only fails the process
//! when `BENCH_TIER3_STORAGE_MODES_STRICT=1`.

use std::process::ExitCode;

use serde_json::{json, Value as Json};

use super::evidence::{
    arm_record, assert_storage_mode, assert_vector_path, counted_run, explain_tokens,
};
use super::profiles::{Profile, Sampling};
use super::queries::{self, Query, Role};
use super::{amplification, expected, sampling, setup, wide, Db};

fn rendered_equal(left: &[Vec<cassie::types::Value>], right: &[Vec<cassie::types::Value>]) -> bool {
    expected::fingerprint(left) == expected::fingerprint(right)
}

/// Pre-timing checks and evidence for one query; returns the verified
/// fingerprint, or `Err` when an arm could not run (for example a budget
/// denial), which is recorded rather than fatal.
fn verify(db: &Db, query: &Query, options: &Sampling) -> Result<((usize, u64, u64), Json), String> {
    let heap_rows = db
        .try_exec(&query.heap_sql)
        .map_err(|error| format!("heap arm: {error}"))?;
    let column_rows = db
        .try_exec(&query.col_sql)
        .map_err(|error| format!("column arm: {error}"))?;
    assert!(
        !heap_rows.is_empty(),
        "{}: baseline must have rows",
        query.name
    );
    assert!(
        rendered_equal(&heap_rows, &column_rows),
        "{}: heap and column results differ",
        query.name
    );
    let fingerprint = expected::fingerprint(&heap_rows);
    drop(column_rows);
    drop(heap_rows);
    if let Some(oracle) = query.oracle_rows() {
        assert_eq!(
            expected::fingerprint(&oracle),
            fingerprint,
            "{}: independent oracle",
            query.name
        );
    }
    let heap_tokens = explain_tokens(db, &query.heap_sql)?;
    let column_tokens = explain_tokens(db, &query.col_sql)?;
    if query.single_table {
        assert_storage_mode(&heap_tokens, "row-store", &query.name);
        assert_storage_mode(&column_tokens, "column-store", &query.name);
    }
    if options.gate.require_vector {
        assert_vector_path(&column_tokens, &query.name);
    }
    let heap_counters = counted_run(db, &query.heap_sql)?;
    let column_counters = counted_run(db, &query.col_sql)?;
    Ok((
        fingerprint,
        json!({
            "result_rows": heap_counters["rows"],
            "heap": arm_record(&heap_tokens, &heap_counters),
            "column": arm_record(&column_tokens, &column_counters),
            "vector_tokens_asserted": options.gate.require_vector,
        }),
    ))
}

fn run_query(db: &Db, query: &Query, options: &Sampling) -> Json {
    let (fingerprint, evidence) = match verify(db, query, options) {
        Ok(verified) => verified,
        Err(error) => {
            println!(
                "storage_modes_query_skipped query={} error={error}",
                query.name
            );
            return json!({"name": query.name, "role": query.role.as_str(), "skipped": error});
        }
    };
    let control = options.control && query.role == Role::Primary;
    let series = sampling::alternate(
        db,
        &query.heap_sql,
        &query.col_sql,
        fingerprint,
        options.warmup,
        options.samples,
        control,
    );
    let analysis = match series {
        Ok(series) => sampling::analyze(&series, options.bootstrap),
        Err(error) => {
            return json!({"name": query.name, "role": query.role.as_str(), "evidence": evidence, "failed": error})
        }
    };
    println!(
        "storage_modes_query name={} role={} median_ratio={:.3} ci95=[{:.3},{:.3}] noise_band={:.3} gate_pass={}",
        query.name,
        query.role.as_str(),
        analysis["paired_ratio_heap_over_column"]["median"].as_f64().unwrap_or(0.0),
        analysis["paired_ratio_heap_over_column"]["ci95_lower"].as_f64().unwrap_or(0.0),
        analysis["paired_ratio_heap_over_column"]["ci95_upper"].as_f64().unwrap_or(0.0),
        analysis["noise_band"].as_f64().unwrap_or(0.0),
        analysis["gate_pass"],
    );
    json!({"name": query.name, "role": query.role.as_str(), "heap_sql": query.heap_sql, "column_sql": query.col_sql, "evidence": evidence, "analysis": analysis})
}

fn lost_primary(results: &[Json]) -> Vec<String> {
    results
        .iter()
        .filter(|result| result["role"] == "primary" && result["analysis"]["gate_pass"] != true)
        .filter_map(|result| result["name"].as_str().map(str::to_owned))
        .collect()
}

pub fn run() -> ExitCode {
    let profile = Profile::from_env();
    let options = Sampling::from_env(&profile);
    println!("storage_modes_profile={profile:?}");
    println!("storage_modes_sampling={options:?}");
    let db = Db::open(
        setup::NativeProfile::Qualification,
        profile.fact_rows * 2,
        profile.query_budget,
        Some((profile.native_total, profile.native_memtable)),
    );
    let setup_ns = wide::seed(&db, profile.fact_rows, profile.wide_rows);
    let results: Vec<Json> = queries::all(profile.fact_rows, profile.wide_rows)
        .iter()
        .filter(|query| options.selects(&query.name))
        .map(|query| run_query(&db, query, &options))
        .collect();
    let amplification = options
        .selects("amplification")
        .then(|| amplification::write_amplification(&db, options.amplification_rows));
    let footprint = (options.footprint && options.selects("footprint")).then(|| {
        amplification::disk_footprint(
            &options.out.with_extension("footprint"),
            options.footprint_rows,
        )
    });
    let lost = lost_primary(&results);
    let report = json!({
        "benchmark": super::BENCHMARK,
        "profile": profile.name,
        "fact_rows": profile.fact_rows,
        "wide_rows": profile.wide_rows,
        "query_memory_budget_bytes": profile.query_budget,
        "native_total_bytes": profile.native_total,
        "native_memtable_bytes": profile.native_memtable,
        "storage_mode": "memory",
        "result_cache": "disabled",
        "warmup": options.warmup,
        "samples": options.samples,
        "control": options.control,
        "measurement_mode": options.measurement,
        "setup_time_ns": setup_ns.to_string(),
        "ratio_definition": "heap_seconds / column_seconds; above 1 means the column arm is faster",
        "advantage_gate": "primary queries: ci95 lower bound of the paired ratio > 1 + heap-versus-heap noise band; 1.25x median is the reported target",
        "queries": results,
        "write_amplification": amplification,
        "disk_footprint": footprint,
        "primary_losses": lost,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    });
    if let Some(parent) = options.out.parent() {
        std::fs::create_dir_all(parent).expect("create output directory");
    }
    std::fs::write(
        &options.out,
        serde_json::to_vec_pretty(&report).expect("serialize report"),
    )
    .expect("write report");
    println!("storage_modes_report={}", options.out.display());
    if lost.is_empty() {
        println!("storage_modes_gate=pass");
        return ExitCode::SUCCESS;
    }
    println!("storage_modes_gate=loss queries={lost:?}");
    if options.gate.strict {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
