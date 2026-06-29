use std::collections::BTreeMap;

use super::{Cassie, QueryResult, QueryError, Value, ColumnMeta};

pub(super) fn diff_projection(
    cassie: &Cassie,
    statement: &crate::sql::ast::DiffProjectionStatement,
) -> Result<QueryResult, QueryError> {
    let left_root = cassie
        .midge
        .root_hash(&statement.left.name)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let right_root = cassie
        .midge
        .root_hash(&statement.right.name)
        .map_err(|error| QueryError::General(error.to_string()))?;
    let columns = diff_columns();

    let Some(left_root) = left_root else {
        return Ok(QueryResult {
            columns,
            rows: vec![terminal_diff_row(
                "__root__",
                "unverifiable",
                None,
                right_root.as_ref().map(|root| root.digest.as_str()),
                "missing-left-root",
            )],
            command: "DIFF PROJECTION".to_string(),
        });
    };
    let Some(right_root) = right_root else {
        return Ok(QueryResult {
            columns,
            rows: vec![terminal_diff_row(
                "__root__",
                "unverifiable",
                Some(left_root.digest.as_str()),
                None,
                "missing-right-root",
            )],
            command: "DIFF PROJECTION".to_string(),
        });
    };

    if left_root.algorithm != right_root.algorithm
        || left_root.range_hash_version != right_root.range_hash_version
        || left_root.root_hash_version != right_root.root_hash_version
    {
        return Ok(QueryResult {
            columns,
            rows: vec![terminal_diff_row(
                "__root__",
                "unverifiable",
                Some(left_root.digest.as_str()),
                Some(right_root.digest.as_str()),
                "incompatible-hash-metadata",
            )],
            command: "DIFF PROJECTION".to_string(),
        });
    }

    if left_root.state != crate::midge::adapter::StoredHashState::Current
        || right_root.state != crate::midge::adapter::StoredHashState::Current
    {
        return Ok(QueryResult {
            columns,
            rows: vec![terminal_diff_row(
                "__root__",
                "unverifiable",
                Some(left_root.digest.as_str()),
                Some(right_root.digest.as_str()),
                "stale-root",
            )],
            command: "DIFF PROJECTION".to_string(),
        });
    }

    if left_root.digest == right_root.digest {
        return Ok(QueryResult {
            columns,
            rows: vec![terminal_diff_row(
                "__root__",
                "equal",
                Some(left_root.digest.as_str()),
                Some(right_root.digest.as_str()),
                "verified",
            )],
            command: "DIFF PROJECTION".to_string(),
        });
    }

    let left = row_hash_map(cassie, &statement.left.name)?;
    let right = row_hash_map(cassie, &statement.right.name)?;
    let row_ids = left
        .keys()
        .chain(right.keys())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    let mut rows = Vec::new();
    let mut next_cursor = None;
    let mut last_emitted = None;
    let limit = statement.limit.unwrap_or(usize::MAX);
    for row_id in row_ids {
        if statement
            .after
            .as_ref()
            .is_some_and(|after| row_id <= *after)
        {
            continue;
        }
        if rows.len() >= limit {
            next_cursor = last_emitted;
            break;
        }
        let before_len = rows.len();
        match (left.get(&row_id), right.get(&row_id)) {
            (Some(left), Some(right)) if left.digest == right.digest => {}
            (Some(left), Some(right)) => rows.push(diff_row(
                &row_id,
                "changed",
                Some(left.digest.as_str()),
                Some(right.digest.as_str()),
                "different-row-hash",
            )),
            (Some(left), None) => rows.push(diff_row(
                &row_id,
                "removed",
                Some(left.digest.as_str()),
                None,
                "missing-right-row",
            )),
            (None, Some(right)) => rows.push(diff_row(
                &row_id,
                "added",
                None,
                Some(right.digest.as_str()),
                "missing-left-row",
            )),
            (None, None) => {}
        }
        if rows.len() > before_len {
            last_emitted = Some(row_id);
        }
    }
    let complete = next_cursor.is_none();
    let cursor = next_cursor.as_deref();
    for row in &mut rows {
        row.push(
            cursor
                .map_or(Value::Null, |value| Value::String(value.to_string())),
        );
        row.push(Value::Bool(complete));
    }
    if rows.is_empty() {
        let mut row = diff_row(
            "__range__",
            "unverifiable",
            Some(left_root.digest.as_str()),
            Some(right_root.digest.as_str()),
            "range-or-root-mismatch-without-row-diff",
        );
        row.push(
            cursor
                .map_or(Value::Null, |value| Value::String(value.to_string())),
        );
        row.push(Value::Bool(complete));
        rows.push(row);
    }
    Ok(QueryResult {
        columns,
        rows,
        command: "DIFF PROJECTION".to_string(),
    })
}

pub(super) fn compare_projection(
    cassie: &Cassie,
    statement: &crate::sql::ast::CompareProjectionStatement,
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
        mismatch_count: mismatches as u64,
        unverifiable_count: unverifiable,
        diagnostic_sample,
        last_error: if state == "equal" {
            None
        } else {
            Some(state.to_string())
        },
    };
    cassie
        .midge
        .put_projection_comparison_report(report.clone())
        .map_err(|error| QueryError::General(error.to_string()))?;
    cassie
        .catalog
        .register_projection_comparison_report(report.clone());
    cassie
        .runtime
        .record_projection_integrity_verification(statement.target.name.clone(), state != "equal");
    Ok(QueryResult {
        columns: vec![
            ColumnMeta::text("target"),
            ColumnMeta::text("state"),
            ColumnMeta::text("root_digest"),
            ColumnMeta::text("manifest_digest"),
            ColumnMeta::from_data_type("mismatches", crate::types::DataType::Int),
            ColumnMeta::text("report_id"),
        ],
        rows: vec![vec![
            Value::String(statement.target.name.clone()),
            Value::String(state.to_string()),
            actual_digest.map_or(Value::Null, Value::String),
            Value::String(manifest_digest.to_string()),
            Value::Int64(mismatches),
            Value::String(report.report_id),
        ]],
        command: "COMPARE PROJECTION".to_string(),
    })
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
    let matches = actual
        .as_str().map_or_else(|| {
            actual
                .as_u64()
                .is_some_and(|value| value.to_string() == expected)
        }, |value| value == expected);
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

fn diff_columns() -> Vec<ColumnMeta> {
    vec![
        ColumnMeta::text("row_id"),
        ColumnMeta::text("change"),
        ColumnMeta::text("left_digest"),
        ColumnMeta::text("right_digest"),
        ColumnMeta::text("state"),
        ColumnMeta::text("next_cursor"),
        ColumnMeta::from_data_type("complete", crate::types::DataType::Boolean),
    ]
}

fn diff_row(
    row_id: &str,
    change: &str,
    left_digest: Option<&str>,
    right_digest: Option<&str>,
    state: &str,
) -> Vec<Value> {
    vec![
        Value::String(row_id.to_string()),
        Value::String(change.to_string()),
        left_digest
            .map_or(Value::Null, |digest| Value::String(digest.to_string())),
        right_digest
            .map_or(Value::Null, |digest| Value::String(digest.to_string())),
        Value::String(state.to_string()),
    ]
}

fn terminal_diff_row(
    row_id: &str,
    change: &str,
    left_digest: Option<&str>,
    right_digest: Option<&str>,
    state: &str,
) -> Vec<Value> {
    let mut row = diff_row(row_id, change, left_digest, right_digest, state);
    row.push(Value::Null);
    row.push(Value::Bool(true));
    row
}
