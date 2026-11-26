//! Paired heap-versus-column harness (`BENCH_TIER3_STORAGE_MODES_MODE=paired`).
//!
//! One process and one fixture serve both arms with the same query budget,
//! cache setting and worker count. Everything before timing is non-timing
//! evidence; a gate loss is recorded and printed, and only fails the process
//! when `BENCH_TIER3_STORAGE_MODES_STRICT=1`.

use std::process::ExitCode;

use serde_json::{json, Value as Json};

use super::evidence::{
    add_vector_delta_tokens, arm_record, assert_storage_mode, assert_vector_path, counted_run,
    explain_tokens,
};
use super::profiles::{Fixture, Profile, Sampling};
use super::queries::{self, Query, Role};
use super::{amplification, expected, fixture_dir::FixtureDir, sampling, setup, wide, Db};

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
    let heap_counters = counted_run(db, &query.heap_sql)?;
    let column_counters = counted_run(db, &query.col_sql)?;
    let mut heap_tokens = heap_tokens;
    let mut column_tokens = column_tokens;
    add_vector_delta_tokens(&mut heap_tokens, &heap_counters);
    add_vector_delta_tokens(&mut column_tokens, &column_counters);
    if options.gate.require_vector {
        assert_vector_path(&column_tokens, &query.name);
    }
    Ok((
        fingerprint,
        json!({
            "result_rows": heap_counters["rows"],
            "result_fingerprint": fingerprint,
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
            eprintln!(
                "storage_modes_query_failed query={} error={error}",
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
            eprintln!(
                "storage_modes_query_failed query={} error={error}",
                query.name
            );
            return json!({"name": query.name, "role": query.role.as_str(), "evidence": evidence, "failed": error});
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
        .filter(|result| result["role"] == "primary" && result["analysis"]["gate_pass"] == false)
        .filter_map(|result| result["name"].as_str().map(str::to_owned))
        .collect()
}

fn report_profile(db: &Db, profile: &Profile, options: &Sampling) -> Json {
    let native = db.cassie.midge.native_memory_configuration();
    let expected_mode = if profile.local { "local" } else { "memory" };
    assert_eq!(native.mode, expected_mode, "native storage mode");
    if let Some((total, memtable)) = profile.native_limits() {
        assert_eq!(native.total, total, "native memory budget");
        assert_eq!(native.memtable_limit, memtable, "native memtable budget");
    }
    json!({
        "benchmark": super::BENCHMARK,
        "profile": profile.name,
        "fixture": options.fixture.as_str(),
        "fact_rows": profile.fact_rows,
        "wide_rows": profile.wide_rows,
        "query_memory_budget_bytes": profile.query_budget,
        "native_total_bytes": native.total,
        "native_memtable_bytes": native.memtable_limit,
        "native_configuration": native,
        "native_budget_source": if profile.local { "local_defaults" } else { "declared_memory_profile" },
        "storage_mode": native.mode,
        "result_cache": "disabled",
        "warmup": options.warmup,
        "samples": options.samples,
        "control": options.control,
        "measurement_mode": options.measurement,
        "query_filter": options.filter,
        "ratio_definition": "heap_seconds / column_seconds; above 1 means the column arm is faster",
        "advantage_gate": "primary queries: ci95 lower bound of the paired ratio > 1 + heap-versus-heap noise band; 1.25x median is the reported target",
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

fn write_report(options: &Sampling, report: &Json) {
    if let Some(parent) = options
        .out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).expect("create output directory");
    }
    std::fs::write(
        &options.out,
        serde_json::to_vec_pretty(report).expect("serialize report"),
    )
    .expect("write report");
    println!("storage_modes_report={}", options.out.display());
}

fn primary_has_qualified_path(result: &Json) -> bool {
    let column = &result["evidence"]["column"];
    let tokens = &column["tokens"];
    let reads = &column["counters"]["column_vector_reads"];
    tokens["storage_mode"] == "column-store"
        && tokens["fallback_reason"] == "none"
        && tokens["column_vector"] == "eligible"
        && reads["batches"].as_u64().is_some_and(|batches| batches > 0)
        && reads["row_documents_built"] == 0
}

fn advantage_status(
    results: &[Json],
    profile: &Profile,
    options: &Sampling,
    failures: &[&str],
    lost: &[String],
) -> &'static str {
    if !failures.is_empty() {
        return "incomplete";
    }
    let primary = || results.iter().filter(|result| result["role"] == "primary");
    match primary().count() {
        0 => "not_run",
        1 => "partial",
        _ if !(profile.is_advantage_profile() && options.measurement && options.control) => {
            "diagnostic"
        }
        _ if !primary().all(primary_has_qualified_path) => "unqualified_path",
        _ if lost.is_empty() => "pass",
        _ => "loss",
    }
}

fn run_optional_probes(
    db: &Db,
    profile: &Profile,
    options: &Sampling,
    amplification: bool,
    footprint: bool,
    report: &mut Json,
) {
    if amplification {
        report["run_status"] = json!("amplification_incomplete");
        write_report(options, report);
        report["write_amplification"] =
            amplification::write_amplification(db, options.amplification_rows, profile.local);
    }
    if footprint {
        report["run_status"] = json!("footprint_incomplete");
        write_report(options, report);
        report["disk_footprint"] = amplification::disk_footprint(
            &options.out.with_extension("footprint"),
            options.footprint_rows,
        );
    }
}

pub fn run() -> ExitCode {
    let fixture = Fixture::from_env();
    let profile = Profile::from_env(fixture);
    let options = Sampling::from_env(&profile, fixture);
    let selected: Vec<Query> = queries::all(profile.fact_rows, profile.wide_rows)
        .into_iter()
        .filter(|query| fixture.includes_query(&query.name) && options.selects(&query.name))
        .collect();
    let amplification = fixture.includes_narrow() && options.selects("amplification");
    let footprint = fixture.includes_narrow() && options.footprint && options.selects("footprint");
    assert!(
        !selected.is_empty() || amplification || footprint,
        "no paired workloads match fixture {} and filters {:?}",
        fixture.as_str(),
        options.filter,
    );

    println!("storage_modes_profile={profile:?}");
    println!("storage_modes_sampling={options:?}");
    let local_path = profile.local.then(|| {
        std::env::temp_dir().join(format!("cassie-storage-modes-{}", uuid::Uuid::new_v4()))
    });
    let _directory = local_path.as_ref().map(FixtureDir::new);
    let db = Db::open(
        setup::NativeProfile::Qualification,
        profile
            .fact_rows
            .max(profile.wide_rows)
            .checked_mul(2)
            .expect("result row limit"),
        profile.query_budget,
        profile.native_limits(),
        local_path.as_deref(),
    );
    let mut report = report_profile(&db, &profile, &options);
    let setup_ns = match wide::seed(
        &db,
        profile.fact_rows,
        profile.wide_rows,
        fixture,
        profile.local,
    ) {
        Ok(elapsed) => elapsed,
        Err(error) => {
            eprintln!("storage_modes_setup_failed={error}");
            report["setup_error"] = json!(error);
            report["run_status"] = json!("setup_failed");
            report["advantage_status"] = json!("not_run");
            write_report(&options, &report);
            return ExitCode::FAILURE;
        }
    };
    let results: Vec<Json> = selected
        .iter()
        .map(|query| run_query(&db, query, &options))
        .collect();
    let failures: Vec<&str> = results
        .iter()
        .filter(|result| result["analysis"].is_null())
        .filter_map(|result| result["name"].as_str())
        .collect();
    let lost = lost_primary(&results);
    let status = advantage_status(&results, &profile, &options, &failures, &lost);
    let query_failed = !failures.is_empty();
    report["setup_time_ns"] = json!(setup_ns.to_string());
    report["advantage_status"] = json!("incomplete");
    report["query_failures"] = json!(failures);
    report["queries"] = json!(results);
    report["write_amplification"] = Json::Null;
    report["disk_footprint"] = Json::Null;
    report["primary_losses"] = json!(lost);
    run_optional_probes(
        &db,
        &profile,
        &options,
        amplification,
        footprint,
        &mut report,
    );
    report["run_status"] = json!(if query_failed {
        "incomplete"
    } else {
        "complete"
    });
    report["advantage_status"] = json!(status);
    write_report(&options, &report);
    println!("storage_modes_gate={status} queries={lost:?}");
    if !query_failed && (!options.gate.strict || status == "pass") {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
