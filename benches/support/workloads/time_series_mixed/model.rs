use std::collections::BTreeMap;

use cassie::app::{ProjectionReplayBatch, ProjectionReplayEvent};
use serde::Serialize;

pub const SOURCE_IDENTITY: &str = "tier5-time-series-event-stream-v1";
pub const WRITER_BATCH_EVENTS: usize = 128;
pub const RETENTION_AT: &str = "2026-01-15T00:00:00Z";
pub const RETENTION_BOUNDARY: &str = "2026-01-08T00:00:00.000000Z";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EventRow {
    pub id: i64,
    pub device: String,
    pub event_at: String,
    pub amount: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Mutation {
    pub event_id: String,
    pub position: u64,
    pub checkpoint: String,
    pub row: EventRow,
}

pub fn initial_rows(count: usize, device_count: usize) -> BTreeMap<i64, EventRow> {
    (1..=count)
        .map(|row_id| {
            let row = initial_row(row_id, device_count);
            (row.id, row)
        })
        .collect()
}

pub fn initial_row(row_id: usize, device_count: usize) -> EventRow {
    let date = 1 + ((row_id - 1) % 14);
    let hour = ((row_id - 1) / 14) % 24;
    let minute = row_id % 60;
    EventRow {
        id: i64::try_from(row_id).expect("seed row id fits i64"),
        device: device_name((row_id - 1) % device_count),
        event_at: format!("2026-01-{date:02}T{hour:02}:{minute:02}:00.000000Z"),
        amount: i64::try_from((row_id.wrapping_mul(37)) % 10_000).expect("seed amount fits i64"),
    }
}

pub fn new_row(row_id: u64, device_count: usize) -> EventRow {
    let slot = row_id % 86_400;
    let hour = slot / 3_600;
    let minute = (slot / 60) % 60;
    let second = slot % 60;
    EventRow {
        id: i64::try_from(row_id).expect("new row id fits i64"),
        device: device_name(
            usize::try_from(row_id % u64::try_from(device_count).expect("device count fits u64"))
                .expect("device index fits usize"),
        ),
        event_at: format!("2026-01-14T{hour:02}:{minute:02}:{second:02}.000000Z"),
        amount: i64::try_from((row_id.wrapping_mul(37)) % 10_000).expect("new amount fits i64"),
    }
}

pub fn correction(row_id: usize, device_count: usize) -> EventRow {
    let mut row = initial_row(row_id, device_count);
    row.event_at = "2026-01-01T00:00:00.000000Z".to_owned();
    row.amount += 100_000;
    row
}

pub fn device_name(index: usize) -> String {
    format!("device-{index:04}")
}

pub fn mutation(row: EventRow, position: u64, event_id: String) -> Mutation {
    Mutation {
        event_id,
        position,
        checkpoint: format!("checkpoint-{position:012}"),
        row,
    }
}

pub fn replay_batch(
    projection: &str,
    batch_id: &str,
    mutations: &[Mutation],
) -> ProjectionReplayBatch {
    ProjectionReplayBatch {
        projection: projection.to_owned(),
        source_identity: SOURCE_IDENTITY.to_owned(),
        batch_id: batch_id.to_owned(),
        lag: 0,
        events: mutations
            .iter()
            .map(|mutation| ProjectionReplayEvent {
                event_id: mutation.event_id.clone(),
                checkpoint: mutation.checkpoint.clone(),
                position: Some(mutation.position),
                document_id: format!("row-{}", mutation.row.id),
                payload: Some(serde_json::json!({
                    "id": mutation.row.id,
                    "device": mutation.row.device,
                    "event_at": mutation.row.event_at,
                    "amount": mutation.row.amount,
                })),
            })
            .collect(),
    }
}

pub fn apply_through_checkpoint(
    initial: &BTreeMap<i64, EventRow>,
    committed: &[Mutation],
    checkpoint_position: u64,
    retention_applied: bool,
) -> BTreeMap<i64, EventRow> {
    let mut rows = initial.clone();
    for mutation in committed
        .iter()
        .filter(|mutation| mutation.position <= checkpoint_position)
    {
        rows.insert(mutation.row.id, mutation.row.clone());
    }
    if retention_applied {
        rows.retain(|_, row| row.event_at.as_str() >= RETENTION_BOUNDARY);
    }
    rows
}

pub fn checkpoint_position(checkpoint: &str) -> Option<u64> {
    checkpoint
        .strip_prefix("checkpoint-")
        .and_then(|suffix| suffix.parse().ok())
}

/// Two-day dashboard windows, `[day, day + 2)` for days 1 through 13, so a page
/// reads a bounded slice of one device partition instead of all fourteen days.
pub const RANGE_WINDOWS: usize = 13;

pub fn window_bounds(window: usize) -> (String, String) {
    let first = 1 + window % RANGE_WINDOWS;
    (
        format!("2026-01-{first:02}T00:00:00.000000Z"),
        format!("2026-01-{:02}T00:00:00.000000Z", first + 2),
    )
}

pub fn oracle_pages(
    rows: &BTreeMap<i64, EventRow>,
    device: &str,
    window: usize,
    limit: usize,
) -> Vec<EventRow> {
    let (start, end) = window_bounds(window);
    let mut page = rows
        .values()
        .filter(|row| {
            row.device == device
                && row.event_at.as_str() >= start.as_str()
                && row.event_at.as_str() < end.as_str()
        })
        .cloned()
        .collect::<Vec<_>>();
    page.sort_by(|left, right| {
        left.event_at
            .cmp(&right.event_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    page.truncate(limit);
    page
}

/// Campaign timeline in seconds. The qualification contract is 100%; a smaller
/// `CASSIE_BENCH_TS_MIXED_TIME_SCALE_PERCENT` exists only for harness smoke runs
/// and is recorded in every raw evidence file.
pub fn time_scale_percent() -> u64 {
    std::env::var("CASSIE_BENCH_TS_MIXED_TIME_SCALE_PERCENT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|percent| (1..=100).contains(percent))
        .unwrap_or(100)
}

pub fn scaled_seconds(contract_seconds: u64) -> u64 {
    (contract_seconds.saturating_mul(time_scale_percent()) / 100).max(1)
}

pub fn scaled(contract_seconds: u64) -> std::time::Duration {
    std::time::Duration::from_secs(scaled_seconds(contract_seconds))
}

pub const INGEST_SECONDS: u64 = 60;
pub const READER_SECONDS: u64 = 120;
pub const REFRESH_AT_SECONDS: u64 = 120;
pub const CORRECTION_AT_SECONDS: u64 = 180;
pub const RETENTION_AT_SECONDS: u64 = 300;
pub const MIXED_SECONDS: u64 = 600;
