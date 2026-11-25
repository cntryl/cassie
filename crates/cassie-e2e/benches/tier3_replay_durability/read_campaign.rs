//! Eight closed-store read profiles; provenance/copying and attestation are untimed.
#[path = "read_campaign/index_attestation.rs"]
mod index_attestation;
#[path = "read_campaign/read_oracle.rs"]
mod read_oracle;
#[path = "read_campaign/store_copy.rs"]
mod store_copy;
use cassie::app::{Cassie, CassieSession};
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::config::{CassieRuntimeConfig, CassieRuntimeLimits, ExecutionResultCacheEnabled};
use cassie::planner::physical::ReadAccessPath;
use cassie::types::Value;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;
use store_copy::{OwnedCopy, ProfileFiles, SourceIdentity, VerifiedSource};

type Result<T> = std::result::Result<T, String>;
const API_COMMIT: &str = "58e6c96ea14c3466da17c256cb2fcaefbd71f940";
const API_TREE: &str = "d051222b580d48266b3ed604f588928bd443e660";
const INDEX: &str = "replay_bench_title_idx";

#[derive(Deserialize)]
struct Manifest {
    format: String,
    reader: SourceIdentity,
    reader_build_profile: String,
    api_writer: SourceIdentity,
    sql_writer: SourceIdentity,
    profiles: Vec<ProfileFiles>,
}

fn load_manifest(path: &Path) -> Result<(Manifest, Vec<VerifiedSource>)> {
    let manifest: Manifest =
        serde_json::from_reader(std::fs::File::open(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    manifest.reader.validate()?;
    if manifest.reader_build_profile.trim().is_empty() {
        return Err("record the reader build profile".into());
    }
    if manifest.format != "cassie-replay-read-v1"
        || manifest.profiles.len() != 8
        || manifest.api_writer.commit != API_COMMIT
        || manifest.api_writer.tree != API_TREE
        || manifest.sql_writer.commit == API_COMMIT
    {
        return Err(
            "campaign requires eight selected A-API/B-SQL profiles and distinct reader provenance"
                .into(),
        );
    }
    let mut profiles = Vec::new();
    let mut cells = BTreeSet::new();
    let mut stores = BTreeSet::new();
    // Reject every missing/foreign closure receipt before any store is copied/opened.
    for files in &manifest.profiles {
        let source = store_copy::load_profile(files, &manifest.api_writer, &manifest.sql_writer)?;
        if !cells.insert((source.result.mode.clone(), source.result.shape.clone())) {
            return Err("duplicate mode/shape in read campaign".into());
        }
        store_copy::claim_original(&source.result.storage_path, &mut stores)?;
        profiles.push(source);
    }
    Ok((manifest, profiles))
}

fn config() -> CassieRuntimeConfig {
    CassieRuntimeConfig {
        limits: CassieRuntimeLimits {
            query_memory_budget_bytes: 64 * 1024 * 1024,
            query_timeout_ms: 120_000,
            execution_result_cache_enabled: ExecutionResultCacheEnabled::disabled(),
            ..CassieRuntimeLimits::default()
        },
        ..CassieRuntimeConfig::default()
    }
}

struct Engine(Cassie);
impl Drop for Engine {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

fn index_metadata(cassie: &Cassie, source: &VerifiedSource) -> Result<serde_json::Value> {
    let collection = cassie.midge.canonical_collection_name("replay_bench");
    let mut catalog = cassie.catalog.list_indexes(&collection);
    catalog.sort_by(|left, right| left.name.cmp(&right.name));
    let mut persisted = cassie
        .midge
        .list_indexes()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|index| index.collection.eq_ignore_ascii_case(&collection))
        .collect::<Vec<_>>();
    persisted.sort_by(|left, right| left.name.cmp(&right.name));
    let catalog_relation = cassie
        .catalog
        .get_collection(&collection)
        .map(|item| item.storage_id);
    let native_relation = cassie
        .midge
        .collection_metadata(&collection)
        .map_err(|error| error.to_string())?
        .map(|item| item.storage_id);
    // Keep raw definitions even when validation reports a gap.
    println!(
        "{}",
        json!({"kind": "index_metadata", "mode": source.result.mode,
        "shape": source.result.shape, "collection": collection,
        "catalog": catalog, "persisted": persisted,
        "catalog_collection_storage_id": catalog_relation,
        "native_collection_storage_id": native_relation})
    );
    let identities = index_attestation::verify_definitions(
        &catalog,
        &persisted,
        catalog_relation.unwrap_or(0),
        native_relation.unwrap_or(0),
    )?;
    if source.result.shape == "indexed" {
        let [index] = catalog.as_slice() else {
            return Err("indexed fixture must have exactly its selected scalar index".into());
        };
        if cassie::catalog::local_name(&index.name) != INDEX
            || index.kind != IndexKind::Scalar
            || index.field != "title"
            || index.unique
            || index.predicate.is_some()
            || index
                .options
                .get(IndexMeta::ACCESS_METHOD_OPTION)
                .map_or("btree", String::as_str)
                != "btree"
        {
            return Err("indexed fixture definition does not match title btree".into());
        }
    } else if !catalog.is_empty() {
        return Err("unindexed fixture unexpectedly contains indexes".into());
    }
    Ok(json!({"catalog": catalog, "persisted": persisted,
        "catalog_collection_storage_id": catalog_relation,
        "native_collection_storage_id": native_relation, "native_identities": identities,
        "definition_comparison_scope": "full persisted definition excluding exactly two hydration storage-ID keys"}))
}

fn counter(metrics: &serde_json::Value, name: &str) -> Result<u64> {
    metrics["read_paths"][name]
        .as_u64()
        .ok_or_else(|| format!("missing read-path counter {name}"))
}
fn delta(before: &serde_json::Value, after: &serde_json::Value, name: &str) -> Result<u64> {
    counter(after, name)?
        .checked_sub(counter(before, name)?)
        .ok_or_else(|| format!("read counter reset: {name}"))
}

fn execute_read(
    cassie: &Cassie,
    session: &CassieSession,
    source: &VerifiedSource,
    round: usize,
    bucket: Option<usize>,
    timed: bool,
    round_ids: &mut BTreeSet<String>,
) -> Result<serde_json::Value> {
    let sql = bucket.map_or_else(
        || "SELECT COUNT(*) FROM replay_bench".to_owned(),
        |bucket| format!("SELECT id, title FROM replay_bench WHERE title = 'title-{bucket}'"),
    );
    let plan = cassie
        .compile_sql_physical_plan_for_diagnostics(&sql)
        .map_err(|error| format!("read plan {sql}: {error}"))?;
    let before = cassie.metrics();
    let start = Instant::now();
    let result = cassie.execute_sql(session, &sql, vec![]);
    // execute_sql returns a fully materialized QueryResult; touching rows is inside timing.
    let row_count = result.as_ref().ok().map(|result| result.rows().len());
    let elapsed_ns = start.elapsed().as_nanos();
    let after = cassie.metrics();
    let checked: Result<serde_json::Value> = (|| {
        let result = result
            .as_ref()
            .map_err(|error| format!("SQLSTATE {}: {error}", error.sql_state()))?;
        let index_ops = [
            "index_seek_scans",
            "prefix_scans",
            "range_scans",
            "ordered_bounded_scans",
        ]
        .into_iter()
        .try_fold(0_u64, |total, key| {
            delta(&before, &after, key).map(|change| total.saturating_add(change))
        })?;
        let scans = delta(&before, &after, "collection_scans")?;
        if let Some(bucket) = bucket {
            read_oracle::validate_bucket(result.rows(), bucket, 6250, round_ids)?;
            if source.result.shape == "indexed" {
                if plan
                    .read
                    .selected_index
                    .as_deref()
                    .map(cassie::catalog::local_name)
                    .as_deref()
                    != Some(INDEX)
                    || plan.read.fallback_reason.is_some()
                    || index_ops == 0
                    || scans != 0
                    || after["read_paths"]["last_index_scan_index"]
                        .as_str()
                        .map(cassie::catalog::local_name)
                        .as_deref()
                        != Some(INDEX)
                {
                    return Err("indexed equality did not attest its actual named index without scan fallback".into());
                }
            } else if plan.read.selected_index.is_some()
                || plan.read.access_path != ReadAccessPath::CollectionScan
                || index_ops != 0
                || scans == 0
            {
                return Err("unindexed equality did not attest actual collection scan".into());
            }
        } else if result.rows() != [vec![Value::Int64(100_000)]] || round_ids.len() != 100_000 {
            return Err(
                "independent count and distinct round IDs must both be exactly 100000".into(),
            );
        }
        Ok(
            json!({"index_ops": index_ops, "collection_scans": scans, "round_unique_ids": round_ids.len()}),
        )
    })();
    let record = json!({"kind": if timed { "read_sample" } else { "untimed_warmup" }, "mode": source.result.mode, "shape": source.result.shape,
        "round": round, "family": if bucket.is_some() { "equality" } else { "count" }, "bucket": bucket, "sql": sql,
        "elapsed_ns": elapsed_ns.to_string(), "rows": row_count, "plan": plan.read,
        "read_paths_before": before["read_paths"], "read_paths_after": after["read_paths"],
        "attestation": checked.as_ref().ok(), "error": checked.as_ref().err()});
    // Keep every attempted SQL/sample in raw stdout even if a later oracle fails.
    println!("{record}");
    checked?;
    Ok(record)
}

fn distribution(samples: &[serde_json::Value], family: &str) -> serde_json::Value {
    let mut values = samples
        .iter()
        .filter(|sample| sample["family"] == family)
        .map(|sample| {
            sample["elapsed_ns"]
                .as_str()
                .expect("sample nanoseconds")
                .parse::<u128>()
                .expect("sample integer")
        })
        .collect::<Vec<_>>();
    values.sort_unstable();
    let n = values.len();
    let p50 = if n.is_multiple_of(2) {
        let sum = values[n / 2 - 1] + values[n / 2];
        format!(
            "{}{}",
            sum / 2,
            if sum.is_multiple_of(2) { "" } else { ".5" }
        )
    } else {
        values[n / 2].to_string()
    };
    json!({"n": n, "p50_ns": p50, "p95_ns": values[(n * 95).div_ceil(100) - 1].to_string(), "p95_rule": "nearest rank; count N=3 p95 is observed maximum"})
}

fn read_store(path: &Path, source: &VerifiedSource) -> Result<serde_json::Value> {
    let engine = Engine(
        Cassie::new_local_with_data_dir_and_config(path, config())
            .map_err(|error| error.to_string())?,
    );
    let cassie = &engine.0;
    cassie.startup().map_err(|error| error.to_string())?;
    let indexes = index_metadata(cassie, source)?;
    let session = cassie.create_session("read_campaign", None);
    // Validate checkpoints before warming/timing; all read-store work is on the copy.
    if source.result.mode.starts_with("sql_") {
        let checkpoint = cassie
            .execute_sql(
                &session,
                "SELECT position FROM replay_bench_checkpoint",
                vec![],
            )
            .map_err(|error| error.to_string())?;
        if checkpoint.rows() != [vec![Value::Int64(100_000)]] {
            return Err("SQL checkpoint mismatch".into());
        }
    } else {
        let projection = cassie.midge.canonical_collection_name("replay_bench");
        let metadata = cassie
            .midge
            .projection_metadata(&projection)
            .map_err(|error| error.to_string())?
            .ok_or("missing projection metadata")?;
        if metadata.source_position != Some(100_000)
            || metadata.source_checkpoint.as_deref() != Some("checkpoint-100000")
            || metadata.applied_event_count != 100_000
        {
            return Err("API checkpoint mismatch".into());
        }
    }
    let mut warmup_ids = BTreeSet::new();
    for bucket in (0..16).map(Some).chain(std::iter::once(None)) {
        execute_read(cassie, &session, source, 0, bucket, false, &mut warmup_ids)?;
    }
    drop(warmup_ids);
    let mut samples = Vec::with_capacity(51);
    for round in 1..=3 {
        let mut round_ids = BTreeSet::new();
        for bucket in (0..16).map(Some).chain(std::iter::once(None)) {
            samples.push(execute_read(
                cassie,
                &session,
                source,
                round,
                bucket,
                true,
                &mut round_ids,
            )?);
        }
    }
    Ok(
        json!({"indexes": indexes, "equality": distribution(&samples, "equality"), "count": distribution(&samples, "count"), "samples": samples}),
    )
}

pub(super) fn print_source_fingerprint(path: &Path) -> Result<()> {
    let fingerprint = store_copy::fingerprint(path)?;
    println!(
        "{}",
        json!({"kind": "source_fingerprint",
        "algorithm": "relative-path-type-length-sha256-v1", "path": path,
        "fingerprint": fingerprint, "closure_attested": false})
    );
    Ok(())
}

pub(super) fn run(path: &Path) -> Result<()> {
    let manifest_sha256 = store_copy::sha256_file(path)?;
    let (manifest, profiles) = load_manifest(path)?;
    println!(
        "{}",
        json!({"kind": "read_campaign", "manifest_sha256": manifest_sha256, "reader": manifest.reader,
        "reader_build_profile_label": manifest.reader_build_profile, "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "profiles": 8, "timed_queries": 408, "identity_scope": "operator labels; verify exact Git/build/producer-process receipts externally"})
    );
    let mut failed = 0;
    for source in profiles {
        let mut verified_after_copy = false;
        let mut verified_after_reader_drop = false;
        let outcome = (|| {
            let copy = OwnedCopy::prepare(&source)?;
            verified_after_copy = true;
            let result = read_store(copy.path(), &source);
            let unchanged = copy.verify_original();
            verified_after_reader_drop = unchanged.is_ok();
            match (result, unchanged) {
                (Ok(result), Ok(())) => Ok(result),
                (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
                (Err(error), Err(source_error)) => Err(format!("{error}; {source_error}")),
            }
        })();
        if outcome.is_err() {
            failed += 1;
        }
        println!(
            "{}",
            json!({"kind": "read_profile", "mode": source.result.mode, "shape": source.result.shape,
            "reader": manifest.reader, "writer": source.writer, "producer_source_label": source.result.source, "receipts": source.files,
            "original_fingerprint_verified_after_copy": verified_after_copy,
            "original_fingerprint_verified_after_reader_drop": verified_after_reader_drop,
            "reader_retirement_scope": "Cassie drop/graceful-close attempt; native quiescence not attested",
            "original_fingerprint_after_reader_drop": if verified_after_reader_drop { Some(&source.files.source_fingerprint) } else { None },
            "query_memory_budget_bytes": 67_108_864, "query_timeout_ms": 120_000, "execution_result_cache_enabled": false,
            "warmup_rounds": 1, "timed_rounds": 3, "status": if outcome.is_ok() { "passed" } else { "gap" },
            "result": outcome.as_ref().ok(), "error": outcome.as_ref().err(),
            "identity_scope": "writer/reader labels require external exact Git and producer-exit verification; no self-attestation"})
        );
    }
    if failed != 0 {
        Err(format!(
            "{failed} read profile(s) failed; retain raw sample/gap records"
        ))
    } else {
        Ok(())
    }
}
