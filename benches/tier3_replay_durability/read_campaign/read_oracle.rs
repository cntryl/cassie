//! Untimed exact bucket/round identity checks shared with focused negative controls.
use cassie::types::Value;
use std::collections::BTreeSet;

pub(crate) fn validate_bucket(
    rows: &[Vec<Value>],
    bucket: usize,
    expected_rows: usize,
    round_ids: &mut BTreeSet<String>,
) -> Result<(), String> {
    if rows.len() != expected_rows {
        return Err(format!(
            "bucket {bucket} must contain exactly {expected_rows} rows"
        ));
    }
    let title = format!("title-{bucket}");
    for row in rows {
        let [Value::String(id), Value::String(actual)] = row.as_slice() else {
            return Err(format!(
                "bucket {bucket} requires non-null text IDs and titles"
            ));
        };
        if actual != &title || id.is_empty() {
            return Err(format!("bucket {bucket} has a wrong title or empty ID"));
        }
        if !round_ids.insert(id.clone()) {
            return Err(format!(
                "duplicate row ID in round at bucket {bucket}: {id}"
            ));
        }
    }
    Ok(())
}
