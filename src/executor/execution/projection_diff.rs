use crate::runtime::QueryExecutionControls;
use std::collections::BTreeMap;

use super::{Cassie, QueryError, QueryResult};

pub(super) fn diff_projection(
    cassie: &Cassie,
    statement: &crate::sql::ast::DiffProjectionStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let left_root = cassie
        .midge
        .root_hash(&statement.left.name)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let right_root = cassie
        .midge
        .root_hash(&statement.right.name)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let (left_root, right_root) = match (left_root.as_ref(), right_root.as_ref()) {
        (Some(left), Some(right)) => (left, right),
        (None, right) => {
            return terminal_diff_result(
                controls,
                "unverifiable",
                None,
                right.map(|root| root.digest.as_str()),
                "missing-left-root",
            )
        }
        (Some(left), None) => {
            return terminal_diff_result(
                controls,
                "unverifiable",
                Some(&left.digest),
                None,
                "missing-right-root",
            )
        }
    };
    if let Some(result) = root_terminal_result(controls, left_root, right_root)? {
        return Ok(result);
    }

    let left = row_hash_map(cassie, &statement.left.name)?;
    let right = row_hash_map(cassie, &statement.right.name)?;
    let slots = crate::executor::retained_memory::add(left.len(), right.len())?;
    let _key_memory = controls.reserve_query_memory(crate::executor::retained_memory::mul(
        slots,
        std::mem::size_of::<&String>(),
    )?)?;
    let mut row_ids = Vec::new();
    row_ids.try_reserve_exact(slots).map_err(|error| {
        crate::app::CassieError::ResourceLimit(format!(
            "unable to retain projection diff keys: {error}"
        ))
    })?;
    row_ids.extend(left.keys().chain(right.keys()));
    row_ids.sort_unstable();
    row_ids.dedup();

    let limit = statement.limit.unwrap_or(usize::MAX);
    let mut count = 0;
    let mut last_emitted = None;
    let mut next_cursor = None;
    for row_id in row_ids.iter().copied() {
        super::check_timeout(controls)?;
        if statement
            .after
            .as_deref()
            .is_some_and(|after| row_id.as_str() <= after)
        {
            continue;
        }
        if count >= limit {
            next_cursor = last_emitted;
            break;
        }
        if diff_row_change(left.get(row_id), right.get(row_id)).is_some() {
            count += 1;
            last_emitted = Some(row_id.as_str());
        }
    }
    let complete = next_cursor.is_none();
    let mut result = diff_result_builder(controls)?;
    let mut emitted = 0;
    for row_id in row_ids {
        super::check_timeout(controls)?;
        if emitted >= count {
            break;
        }
        if statement
            .after
            .as_deref()
            .is_some_and(|after| row_id.as_str() <= after)
        {
            continue;
        }
        if let Some(change) = diff_row_change(left.get(row_id), right.get(row_id)) {
            push_diff_row(
                &mut result,
                controls,
                DiffRow {
                    row_id,
                    change: change.change,
                    left_digest: change.left_digest,
                    right_digest: change.right_digest,
                    state: change.state,
                    cursor: next_cursor,
                    complete,
                },
            )?;
            emitted += 1;
        }
    }
    if emitted == 0 {
        push_diff_row(
            &mut result,
            controls,
            DiffRow {
                row_id: "__range__",
                change: "unverifiable",
                left_digest: Some(&left_root.digest),
                right_digest: Some(&right_root.digest),
                state: "range-or-root-mismatch-without-row-diff",
                cursor: next_cursor,
                complete,
            },
        )?;
    }
    Ok(result.finish()?)
}

fn root_terminal_result(
    controls: &QueryExecutionControls,
    left: &crate::midge::adapter::RootHashRecord,
    right: &crate::midge::adapter::RootHashRecord,
) -> Result<Option<QueryResult>, QueryError> {
    let outcome = if left.algorithm != right.algorithm
        || left.range_hash_version != right.range_hash_version
        || left.root_hash_version != right.root_hash_version
    {
        Some(("unverifiable", "incompatible-hash-metadata"))
    } else if left.state != crate::midge::adapter::StoredHashState::Current
        || right.state != crate::midge::adapter::StoredHashState::Current
    {
        Some(("unverifiable", "stale-root"))
    } else if left.digest == right.digest {
        Some(("equal", "verified"))
    } else {
        None
    };
    outcome
        .map(|(change, state)| {
            terminal_diff_result(
                controls,
                change,
                Some(&left.digest),
                Some(&right.digest),
                state,
            )
        })
        .transpose()
}

fn terminal_diff_result(
    controls: &QueryExecutionControls,
    change: &str,
    left_digest: Option<&str>,
    right_digest: Option<&str>,
    state: &str,
) -> Result<QueryResult, QueryError> {
    let mut result = diff_result_builder(controls)?;
    push_diff_row(
        &mut result,
        controls,
        DiffRow {
            row_id: "__root__",
            change,
            left_digest,
            right_digest,
            state,
            cursor: None,
            complete: true,
        },
    )?;
    Ok(result.finish()?)
}

fn diff_result_builder(
    controls: &QueryExecutionControls,
) -> Result<super::QueryResultBuilder<'_>, QueryError> {
    super::command_output::columns(
        "DIFF PROJECTION",
        &[
            ("row_id", crate::types::DataType::Text),
            ("change", crate::types::DataType::Text),
            ("left_digest", crate::types::DataType::Text),
            ("right_digest", crate::types::DataType::Text),
            ("state", crate::types::DataType::Text),
            ("next_cursor", crate::types::DataType::Text),
            ("complete", crate::types::DataType::Boolean),
        ],
        controls,
    )
}

struct DiffRow<'a> {
    row_id: &'a str,
    change: &'a str,
    left_digest: Option<&'a str>,
    right_digest: Option<&'a str>,
    state: &'a str,
    cursor: Option<&'a str>,
    complete: bool,
}

fn push_diff_row(
    result: &mut super::QueryResultBuilder<'_>,
    controls: &QueryExecutionControls,
    row: DiffRow<'_>,
) -> Result<(), QueryError> {
    use super::command_output::Cell;
    super::command_output::push_row(
        result,
        &[
            Cell::Text(row.row_id),
            Cell::Text(row.change),
            Cell::OptionalText(row.left_digest),
            Cell::OptionalText(row.right_digest),
            Cell::Text(row.state),
            Cell::OptionalText(row.cursor),
            Cell::Bool(row.complete),
        ],
        controls,
    )
}

struct DiffChange<'a> {
    change: &'static str,
    left_digest: Option<&'a str>,
    right_digest: Option<&'a str>,
    state: &'static str,
}

fn diff_row_change<'a>(
    left: Option<&'a crate::midge::adapter::RowHashRecord>,
    right: Option<&'a crate::midge::adapter::RowHashRecord>,
) -> Option<DiffChange<'a>> {
    match (left, right) {
        (Some(left), Some(right)) if left.digest == right.digest => None,
        (Some(left), Some(right)) => Some(DiffChange {
            change: "changed",
            left_digest: Some(&left.digest),
            right_digest: Some(&right.digest),
            state: "different-row-hash",
        }),
        (Some(left), None) => Some(DiffChange {
            change: "removed",
            left_digest: Some(&left.digest),
            right_digest: None,
            state: "missing-right-row",
        }),
        (None, Some(right)) => Some(DiffChange {
            change: "added",
            left_digest: None,
            right_digest: Some(&right.digest),
            state: "missing-left-row",
        }),
        (None, None) => None,
    }
}

pub(super) fn compare_projection(
    cassie: &Cassie,
    statement: &crate::sql::ast::CompareProjectionStatement,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, QueryError> {
    let manifest: serde_json::Value = serde_json::from_str(&statement.manifest)
        .map_err(|error| QueryError::General(format!("invalid projection manifest: {error}")))?;
    let manifest_digest = manifest
        .get("root_digest")
        .or_else(|| manifest.get("digest"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            QueryError::General("projection manifest requires root_digest or digest".to_string())
        })?;
    let root = cassie
        .midge
        .root_hash(&statement.target.name)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let mut compatibility_status = "compatible".to_string();
    let mut diagnostic_sample = Vec::new();
    let (state, mismatches, unverifiable, actual_digest) = match root {
        Some(root)
            if root.state == crate::midge::adapter::StoredHashState::Current
                && manifest_hash_metadata_matches(&manifest, &root, &mut compatibility_status)
                && root.digest == manifest_digest =>
        {
            ("equal", 0_i64, 0_u64, Some(root.digest))
        }
        Some(root)
            if root.state == crate::midge::adapter::StoredHashState::Current
                && manifest_hash_metadata_matches(&manifest, &root, &mut compatibility_status) =>
        {
            diagnostic_sample.push("root-digest-mismatch".to_string());
            ("mismatch", 1_i64, 0_u64, Some(root.digest))
        }
        Some(root) if root.state == crate::midge::adapter::StoredHashState::Current => {
            diagnostic_sample.push(compatibility_status.clone());
            ("unverifiable", 1_i64, 1_u64, Some(root.digest))
        }
        Some(root) => {
            compatibility_status = "stale-root".to_string();
            diagnostic_sample.push(stored_hash_state(&root.state).to_string());
            ("unverifiable", 1_i64, 1_u64, Some(root.digest))
        }
        None => {
            compatibility_status = "missing-root".to_string();
            diagnostic_sample.push("missing-root".to_string());
            ("unverifiable", 1_i64, 1_u64, None)
        }
    };
    let report = crate::catalog::ProjectionComparisonReportMeta {
        report_id: format!("projection-comparison-{}", uuid::Uuid::new_v4()),
        created_ms: now_ms(),
        target: statement.target.name.clone(),
        target_version_id: statement.target.version_id.clone(),
        state: state.to_string(),
        compatibility_status,
        root_digest: actual_digest.clone(),
        manifest_digest: Some(manifest_digest.to_string()),
        mismatch_count: mismatches.unsigned_abs(),
        unverifiable_count: unverifiable,
        diagnostic_sample,
        last_error: if state == "equal" {
            None
        } else {
            Some(state.to_string())
        },
    };
    let mut result = super::command_output::columns(
        "COMPARE PROJECTION",
        &[
            ("target", crate::types::DataType::Text),
            ("state", crate::types::DataType::Text),
            ("root_digest", crate::types::DataType::Text),
            ("manifest_digest", crate::types::DataType::Text),
            ("mismatches", crate::types::DataType::Int),
            ("report_id", crate::types::DataType::Text),
        ],
        controls,
    )?;
    use super::command_output::Cell;
    super::command_output::push_row(
        &mut result,
        &[
            Cell::Text(&statement.target.name),
            Cell::Text(state),
            Cell::OptionalText(actual_digest.as_deref()),
            Cell::Text(manifest_digest),
            Cell::Int(mismatches),
            Cell::Text(&report.report_id),
        ],
        controls,
    )?;
    let output = result.finish()?;
    let gated_projection = report.target.clone();
    cassie
        .midge
        .with_collection_gates(std::slice::from_ref(&gated_projection), || {
            cassie
                .midge
                .put_projection_comparison_report(&report)
                .map_err(|error| QueryError::General(error.to_string()))?;
            cassie
                .catalog
                .register_projection_comparison_report(report.clone());
            Ok::<(), QueryError>(())
        })?;
    cassie
        .runtime
        .record_projection_integrity_verification(statement.target.name.clone(), state != "equal");
    Ok(output)
}

fn manifest_hash_metadata_matches(
    manifest: &serde_json::Value,
    root: &crate::midge::adapter::RootHashRecord,
    compatibility_status: &mut String,
) -> bool {
    manifest_field_matches(manifest, "algorithm", &root.algorithm, compatibility_status)
        && manifest_field_matches(
            manifest,
            "digest_length",
            &root.digest_length.to_string(),
            compatibility_status,
        )
        && manifest_field_matches(
            manifest,
            "canonical_encoder_version",
            &root.canonical_encoder_version.to_string(),
            compatibility_status,
        )
        && manifest_field_matches(
            manifest,
            "row_hash_version",
            &root.row_hash_version.to_string(),
            compatibility_status,
        )
        && manifest_field_matches(
            manifest,
            "range_hash_version",
            &root.range_hash_version.to_string(),
            compatibility_status,
        )
        && manifest_field_matches(
            manifest,
            "root_hash_version",
            &root.root_hash_version.to_string(),
            compatibility_status,
        )
}

fn manifest_field_matches(
    manifest: &serde_json::Value,
    key: &str,
    expected: &str,
    compatibility_status: &mut String,
) -> bool {
    let Some(actual) = manifest.get(key) else {
        *compatibility_status = format!("incompatible-missing-{key}");
        return false;
    };
    let matches = actual.as_str().map_or_else(
        || {
            actual
                .as_u64()
                .is_some_and(|value| value.to_string() == expected)
        },
        |value| value == expected,
    );
    if !matches {
        *compatibility_status = format!("incompatible-{key}");
    }
    matches
}

fn stored_hash_state(state: &crate::midge::adapter::StoredHashState) -> &'static str {
    match state {
        crate::midge::adapter::StoredHashState::Current => "current",
        crate::midge::adapter::StoredHashState::Stale => "stale",
        crate::midge::adapter::StoredHashState::Incomplete => "incomplete",
        crate::midge::adapter::StoredHashState::Incompatible => "incompatible",
        crate::midge::adapter::StoredHashState::Empty => "empty",
        crate::midge::adapter::StoredHashState::Tombstone => "tombstone",
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

fn row_hash_map(
    cassie: &Cassie,
    collection: &str,
) -> Result<BTreeMap<String, crate::midge::adapter::RowHashRecord>, QueryError> {
    Ok(cassie
        .midge
        .list_row_hashes(collection)
        .map_err(|error| QueryError::General(error.to_string()))?
        .into_iter()
        .map(|record| (record.row_id.clone(), record))
        .collect())
}
