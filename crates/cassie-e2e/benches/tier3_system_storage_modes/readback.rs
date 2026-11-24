//! Authoritative data-only error readback. No SQL query, statistics refresh or
//! storage write is performed after a COPY error.
use super::{Db, FixtureKind};
use cassie::app::CassieError;
use cassie::types::Value;
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

impl FixtureKind {
    fn columns(self) -> &'static [&'static str] {
        match self {
            Self::Fact => &[
                "id", "grp", "cat", "amount", "qty", "flag", "note", "dim_id",
            ],
            Self::Dimension => &["dim_id", "region", "weight"],
        }
    }

    fn expected_values(self, id: usize) -> Vec<Json> {
        match self {
            Self::Fact => super::super::expected::fact_row(id)
                .into_iter()
                .map(|value| match value {
                    Value::Null => Json::Null,
                    Value::Bool(value) => json!(value),
                    Value::Int64(value) => json!(value),
                    Value::Float64(value) => json!(value),
                    Value::String(value) => json!(value),
                    other => panic!("unexpected independent fixture value: {other:?}"),
                })
                .collect(),
            Self::Dimension => vec![
                json!(id),
                json!(format!("region{}", id % 10)),
                json!(id % 50),
            ],
        }
    }
}

fn update(hash: &mut Sha256, key: i64, values: &[Json]) -> Result<(), CassieError> {
    let encoded =
        serde_json::to_vec(values).map_err(|error| CassieError::Parse(error.to_string()))?;
    hash.update(key.to_be_bytes());
    hash.update(
        u64::try_from(encoded.len())
            .expect("fixture row length fits u64")
            .to_be_bytes(),
    );
    hash.update(encoded);
    Ok(())
}

fn lowercase_hex(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn expected_digest(kind: FixtureKind, rows: usize) -> Result<String, CassieError> {
    let mut hash = Sha256::new();
    for id in 0..rows {
        update(
            &mut hash,
            i64::try_from(id).expect("fixture PK fits i64"),
            &kind.expected_values(id),
        )?;
    }
    Ok(lowercase_hex(&hash.finalize()))
}

pub(super) fn authoritative(
    db: &Db,
    table: &str,
    kind: FixtureKind,
    acknowledged: usize,
    requested_end: usize,
) -> Result<Json, CassieError> {
    let columns = kind.columns();
    let mut rows = db
        .cassie
        .midge
        .scan_documents(table)?
        .into_iter()
        .map(|document| {
            let key = document
                .payload
                .get(columns[0])
                .and_then(Json::as_i64)
                .ok_or_else(|| {
                    CassieError::Execution(format!(
                        "fixture row '{}' lacks primary key '{}'",
                        document.id, columns[0]
                    ))
                })?;
            let values = columns
                .iter()
                .map(|field| document.payload.get(*field).cloned().unwrap_or(Json::Null))
                .collect::<Vec<_>>();
            Ok((key, values))
        })
        .collect::<Result<Vec<_>, CassieError>>()?;
    rows.sort_by_key(|(key, _)| *key);
    let mut hash = Sha256::new();
    for (key, values) in &rows {
        update(&mut hash, *key, values)?;
    }
    let actual = lowercase_hex(&hash.finalize());
    let before = expected_digest(kind, acknowledged)?;
    let requested = expected_digest(kind, requested_end)?;
    Ok(
        json!({"row_count": rows.len(), "primary_key_min": rows.first().map(|(key,_)| key), "primary_key_max": rows.last().map(|(key,_)| key), "primary_key_values_sha256": actual, "acknowledged_prefix_sha256": before, "requested_prefix_sha256": requested, "matches_acknowledged_prefix": rows.len() == acknowledged && actual == before, "matches_requested_prefix": rows.len() == requested_end && actual == requested, "source": "authoritative_midge_scan", "sql_queries_issued": 0}),
    )
}
