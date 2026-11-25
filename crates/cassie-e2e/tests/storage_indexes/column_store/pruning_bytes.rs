//! Persisted bytes and selective read counts of authoritative column tables
//! with compressed field values (#1357), measured on fixed fixtures.
//!
//! "Before" is the compact encoding the format stored before value compression
//! (`[type tag][UTF-8 bytes]` for TEXT), computed from the same data; "after"
//! is what the column family holds now.

use super::pruning::{ints, observe, summary_entries};
use super::{env, Env};

const ROWS: usize = 1_280;
const CHUNK: usize = 256;
const PAYLOAD_FIELD: u32 = 2;

fn repetitive(index: usize) -> String {
    format!("{}row-{index:05}", "event-log-line ".repeat(24))
}

fn incompressible(index: usize) -> String {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64 ^ (index as u64 + 1);
    (0..375)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            char::from(b'a' + u8::try_from(state % 26).expect("letter"))
        })
        .collect()
}

fn load(env: &Env, table: &str, payload: impl Fn(usize) -> String) -> Vec<String> {
    env.run(&format!(
        "CREATE TABLE {table} (id INT, payload TEXT) USING columnar"
    ));
    let payloads = (0..ROWS).map(&payload).collect::<Vec<_>>();
    for start in (0..ROWS).step_by(CHUNK) {
        let values = (start..start + CHUNK)
            .map(|index| format!("({index}, '{}')", payloads[index]))
            .collect::<Vec<_>>()
            .join(",");
        env.run(&format!("INSERT INTO {table} VALUES {values}"));
    }
    env.cassie
        .midge
        .rebuild_column_store_summaries_for_diagnostics(table)
        .expect("summaries");
    payloads
}

struct Footprint {
    stored: usize,
    uncompressed: usize,
    compressed_values: usize,
}

fn payload_footprint(env: &Env, table: &str, payloads: &[String]) -> Footprint {
    let prefix = env.field_key(table, PAYLOAD_FIELD, "");
    let entries = env
        .column_entries(table)
        .into_iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .collect::<Vec<_>>();
    assert_eq!(entries.len(), payloads.len());
    let key_bytes = entries.iter().map(|(key, _)| key.len()).sum::<usize>();
    Footprint {
        stored: key_bytes + entries.iter().map(|(_, value)| value.len()).sum::<usize>(),
        uncompressed: key_bytes + payloads.iter().map(|text| 1 + text.len()).sum::<usize>(),
        compressed_values: entries
            .iter()
            .filter(|(_, value)| value.first() == Some(&0x80))
            .count(),
    }
}

#[test]
fn should_report_bytes_saved_with_fewer_selective_reads_on_a_compressible_fixture() {
    // Arrange
    let env = env();
    let payloads = load(&env, "docs", repetitive);
    let sql = "SELECT id, payload FROM docs WHERE id >= 1000";
    let equivalent = "SELECT id, payload FROM docs WHERE id > 999";

    // Act
    let footprint = payload_footprint(&env, "docs", &payloads);
    let summary_bytes = summary_entries(&env, "docs")
        .iter()
        .map(|(key, value)| key.len() + value.len())
        .sum::<usize>();
    let pruned = observe(&env, sql);
    env.cassie
        .midge
        .set_column_pruning_enabled_for_diagnostics(false);
    let unpruned = observe(&env, equivalent);
    println!(
        "compressible: rows={ROWS} before_bytes={} after_bytes={} summary_bytes={summary_bytes} \
         pruned_reads={} unpruned_reads={} pruned_payload_bytes={} unpruned_payload_bytes={}",
        footprint.uncompressed,
        footprint.stored,
        pruned.fields.payload_reads,
        unpruned.fields.payload_reads,
        pruned.fields.payload_bytes,
        unpruned.fields.payload_bytes,
    );

    // Assert
    assert_eq!(footprint.compressed_values, ROWS);
    assert!(
        footprint.stored * 2 < footprint.uncompressed,
        "at least half saved"
    );
    assert!(
        footprint.stored + summary_bytes < footprint.uncompressed,
        "net saving"
    );
    assert_eq!(ints(&pruned.rows, 0), (1_000..1_280).collect::<Vec<_>>());
    assert_eq!(ints(&unpruned.rows, 0), ints(&pruned.rows, 0));
    assert_eq!(pruned.fields.payload_reads, 2 * 512);
    assert_eq!(unpruned.fields.payload_reads, 2 * ROWS as u64);
    assert!(pruned.fields.payload_bytes * 2 < unpruned.fields.payload_bytes);
    assert_eq!(pruned.fields.decodes, pruned.fields.payload_reads);
}

#[test]
fn should_return_compressed_values_unchanged() {
    // Arrange
    let env = env();
    let payloads = load(&env, "docs", repetitive);

    // Act
    let rows = env.rows("SELECT id, payload FROM docs WHERE id IN (0, 641, 1279) ORDER BY id");

    // Assert
    let texts = rows
        .iter()
        .map(|row| match &row[1] {
            cassie::types::Value::String(text) => text.clone(),
            other => panic!("expected text, found {other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        vec![
            payloads[0].clone(),
            payloads[641].clone(),
            payloads[1279].clone()
        ]
    );
}

#[test]
fn should_store_incompressible_values_unchanged_with_no_size_overhead() {
    // Arrange
    let env = env();
    let payloads = load(&env, "noise", incompressible);

    // Act
    let footprint = payload_footprint(&env, "noise", &payloads);
    let read = observe(&env, "SELECT id, payload FROM noise WHERE id >= 1000");

    // Assert
    assert_eq!(footprint.compressed_values, 0);
    assert_eq!(footprint.stored, footprint.uncompressed);
    assert_eq!(read.rows.len(), 280);
    assert_eq!(read.fields.payload_reads, 2 * 512);
}
