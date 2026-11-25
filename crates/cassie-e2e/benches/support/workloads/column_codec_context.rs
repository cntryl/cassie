use std::future::{ready, Ready};

use cassie::app::CassieError;
use cassie::catalog::canonical_relation_name;
use cassie::types::DataType;

use super::context::{bench_document_schema, unindexed_context, BenchContext};

/// Queries executed in each slower compressible column-codec acceptance sample.
pub const COMPRESSIBLE_COLUMN_CODEC_QUERIES_PER_SAMPLE: usize = 256;

/// Queries executed in each fast column-codec acceptance sample.
///
/// These sub-millisecond query paths need a multi-second paired window so shared-runner
/// scheduling noise does not invalidate candidate and baseline together.
pub const FAST_COLUMN_CODEC_QUERIES_PER_SAMPLE: usize = 8_192;

/// Queries executed in each low-cardinality FSST acceptance sample.
pub const FSST_COLUMN_CODEC_QUERIES_PER_SAMPLE: usize = 512;

pub const COMPRESSIBLE_AUTO_SQL: &str =
    "SELECT title, body FROM bench_documents WHERE status = 'approved' AND score >= 90";
pub const COMPRESSIBLE_PLAIN_SQL: &str =
    "SELECT title, body FROM bench_documents_plain WHERE status = 'approved' AND score >= 90";
pub const INCOMPRESSIBLE_AUTO_SQL: &str =
    "SELECT title, body FROM bench_documents_incompressible WHERE title >= '000000000000079c-'";
pub const INCOMPRESSIBLE_PLAIN_SQL: &str =
    "SELECT title, body FROM bench_documents_incompressible_plain WHERE title >= '000000000000079c-'";
pub const ALP_AUTO_SQL: &str = "SELECT score FROM bench_documents_alp WHERE score >= 9.24";
pub const ALP_PLAIN_SQL: &str = "SELECT score FROM bench_documents_alp_plain WHERE score >= 9.24";
pub const FSST_AUTO_SQL: &str = "SELECT body FROM bench_documents_fsst WHERE score = 1";
pub const FSST_PLAIN_SQL: &str = "SELECT body FROM bench_documents_fsst_plain WHERE score = 1";

pub fn column_codec_acceptance_context(rows: usize) -> Ready<Result<BenchContext, CassieError>> {
    ready(column_codec_acceptance_context_now(rows))
}

fn column_codec_acceptance_context_now(rows: usize) -> Result<BenchContext, CassieError> {
    let context = unindexed_context("tier2-column-codec-2k", rows).into_inner()?;
    let compressible = compressible_documents(rows);
    context
        .cassie
        .midge
        .put_documents("bench_documents", compressible.clone())?;
    create_column_index(
        &context,
        "bench_documents",
        "bench_documents_column_idx",
        "title, body, status, score",
    )?;
    assert_compressed_chunks(&context, "bench_documents", "bench_documents_column_idx")?;

    create_bench_collection(&context, "bench_documents_plain", compressible)?;
    create_column_index(
        &context,
        "bench_documents_plain",
        "bench_documents_plain_column_idx",
        "title, body, status, score",
    )?;
    context
        .cassie
        .midge
        .rebuild_column_batches_plain_for_benchmark(
            "bench_documents_plain",
            "bench_documents_plain_column_idx",
        )?;
    assert_plain_chunks(
        &context,
        "bench_documents_plain",
        "bench_documents_plain_column_idx",
    )?;

    let incompressible = incompressible_documents(rows);
    create_bench_collection(
        &context,
        "bench_documents_incompressible",
        incompressible.clone(),
    )?;
    create_column_index(
        &context,
        "bench_documents_incompressible",
        "bench_documents_incompressible_column_idx",
        "title, body",
    )?;
    assert_plain_chunks(
        &context,
        "bench_documents_incompressible",
        "bench_documents_incompressible_column_idx",
    )?;

    create_bench_collection(
        &context,
        "bench_documents_incompressible_plain",
        incompressible,
    )?;
    create_column_index(
        &context,
        "bench_documents_incompressible_plain",
        "bench_documents_incompressible_plain_column_idx",
        "title, body",
    )?;
    context
        .cassie
        .midge
        .rebuild_column_batches_plain_for_benchmark(
            "bench_documents_incompressible_plain",
            "bench_documents_incompressible_plain_column_idx",
        )?;

    add_alp_acceptance_pair(&context, rows)?;
    add_fsst_acceptance_pair(&context, rows)?;

    Ok(context)
}

fn add_fsst_acceptance_pair(context: &BenchContext, rows: usize) -> Result<(), CassieError> {
    let fsst = fsst_documents(rows);
    create_bench_collection(context, "bench_documents_fsst", fsst.clone())?;
    create_column_index(
        context,
        "bench_documents_fsst",
        "bench_documents_fsst_column_idx",
        "body, score",
    )?;
    assert_selected_codec(
        context,
        "bench_documents_fsst",
        "bench_documents_fsst_column_idx",
        "body",
        "fsst",
    )?;
    assert_fsst_storage_savings(
        context,
        "bench_documents_fsst",
        "bench_documents_fsst_column_idx",
        "body",
    )?;

    create_bench_collection(context, "bench_documents_fsst_plain", fsst)?;
    create_column_index(
        context,
        "bench_documents_fsst_plain",
        "bench_documents_fsst_plain_column_idx",
        "body, score",
    )?;
    context
        .cassie
        .midge
        .rebuild_column_batches_plain_for_benchmark(
            "bench_documents_fsst_plain",
            "bench_documents_fsst_plain_column_idx",
        )?;
    assert_selected_codec(
        context,
        "bench_documents_fsst_plain",
        "bench_documents_fsst_plain_column_idx",
        "body",
        "plain",
    )?;

    Ok(())
}

fn add_alp_acceptance_pair(context: &BenchContext, rows: usize) -> Result<(), CassieError> {
    let mut alp_schema = bench_document_schema();
    alp_schema
        .fields
        .iter_mut()
        .find(|field| field.name == "score")
        .ok_or_else(|| CassieError::Execution("missing benchmark score field".to_string()))?
        .data_type = DataType::Float;
    let alp = alp_documents(rows);
    create_bench_collection_with_schema(context, "bench_documents_alp", &alp_schema, alp.clone())?;
    create_column_index(
        context,
        "bench_documents_alp",
        "bench_documents_alp_column_idx",
        "score",
    )?;
    assert_selected_codec(
        context,
        "bench_documents_alp",
        "bench_documents_alp_column_idx",
        "score",
        "alp",
    )?;
    assert_alp_storage_savings(
        context,
        "bench_documents_alp",
        "bench_documents_alp_column_idx",
        "score",
    )?;

    create_bench_collection_with_schema(context, "bench_documents_alp_plain", &alp_schema, alp)?;
    create_column_index(
        context,
        "bench_documents_alp_plain",
        "bench_documents_alp_plain_column_idx",
        "score",
    )?;
    context
        .cassie
        .midge
        .rebuild_column_batches_plain_for_benchmark(
            "bench_documents_alp_plain",
            "bench_documents_alp_plain_column_idx",
        )?;
    assert_selected_codec(
        context,
        "bench_documents_alp_plain",
        "bench_documents_alp_plain_column_idx",
        "score",
        "plain",
    )?;

    Ok(())
}

fn create_bench_collection(
    context: &BenchContext,
    collection: &str,
    documents: Vec<(Option<String>, serde_json::Value)>,
) -> Result<(), CassieError> {
    let schema = bench_document_schema();
    create_bench_collection_with_schema(context, collection, &schema, documents)
}

fn create_bench_collection_with_schema(
    context: &BenchContext,
    collection: &str,
    schema: &cassie::types::Schema,
    documents: Vec<(Option<String>, serde_json::Value)>,
) -> Result<(), CassieError> {
    context
        .cassie
        .midge
        .create_collection(collection, schema.clone())?;
    context.cassie.register_collection(
        canonical_relation_name("postgres", "public", collection),
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
    context.cassie.midge.put_documents(collection, documents)?;
    Ok(())
}

fn assert_selected_codec(
    context: &BenchContext,
    collection: &str,
    index: &str,
    field: &str,
    codec: &str,
) -> Result<(), CassieError> {
    let metadata = context
        .cassie
        .midge
        .get_column_batch_metadata(collection, index)?
        .ok_or_else(|| CassieError::Execution("missing column benchmark metadata".to_string()))?;
    if metadata.segments.iter().any(|segment| {
        segment
            .field_chunks
            .get(field)
            .is_none_or(|chunk| chunk.codec_name != codec)
    }) {
        return Err(CassieError::Execution(format!(
            "benchmark field '{field}' did not select codec '{codec}'"
        )));
    }
    Ok(())
}

fn assert_alp_storage_savings(
    context: &BenchContext,
    collection: &str,
    index: &str,
    field: &str,
) -> Result<(), CassieError> {
    let metadata = context
        .cassie
        .midge
        .get_column_batch_metadata(collection, index)?
        .ok_or_else(|| CassieError::Execution("missing ALP benchmark metadata".to_string()))?;
    if metadata.segments.iter().any(|segment| {
        segment
            .field_chunks
            .get(field)
            .is_none_or(|chunk| chunk.encoded_len.saturating_mul(4) > chunk.decoded_len)
    }) {
        return Err(CassieError::Execution(
            "ALP benchmark chunks did not reduce plain bytes by at least 75%".to_string(),
        ));
    }
    Ok(())
}

fn assert_fsst_storage_savings(
    context: &BenchContext,
    collection: &str,
    index: &str,
    field: &str,
) -> Result<(), CassieError> {
    let metadata = context
        .cassie
        .midge
        .get_column_batch_metadata(collection, index)?
        .ok_or_else(|| CassieError::Execution("missing FSST benchmark metadata".to_string()))?;
    if metadata.segments.iter().any(|segment| {
        segment.field_chunks.get(field).is_none_or(|chunk| {
            chunk.encoded_len.saturating_mul(4) > chunk.decoded_len.saturating_mul(3)
        })
    }) {
        return Err(CassieError::Execution(
            "FSST benchmark chunks did not reduce plain bytes by at least 25%".to_string(),
        ));
    }
    Ok(())
}

fn create_column_index(
    context: &BenchContext,
    collection: &str,
    index: &str,
    fields: &str,
) -> Result<(), CassieError> {
    context.cassie.execute_sql(
        &context.session,
        &format!(
            "CREATE INDEX {index} ON {collection} USING column ({fields}) WITH (segment_size = 256)"
        ),
        vec![],
    )?;
    Ok(())
}

fn assert_plain_chunks(
    context: &BenchContext,
    collection: &str,
    index: &str,
) -> Result<(), CassieError> {
    let metadata = context
        .cassie
        .midge
        .get_column_batch_metadata(collection, index)?
        .ok_or_else(|| CassieError::Execution("missing column benchmark metadata".to_string()))?;
    if metadata.segments.iter().any(|segment| {
        segment
            .field_chunks
            .values()
            .any(|chunk| chunk.codec_name != "plain")
    }) {
        return Err(CassieError::Execution(
            "incompressible benchmark fixture selected a non-plain codec".to_string(),
        ));
    }
    Ok(())
}

fn assert_compressed_chunks(
    context: &BenchContext,
    collection: &str,
    index: &str,
) -> Result<(), CassieError> {
    let metadata = context
        .cassie
        .midge
        .get_column_batch_metadata(collection, index)?
        .ok_or_else(|| CassieError::Execution("missing column benchmark metadata".to_string()))?;
    if metadata.segments.iter().all(|segment| {
        segment
            .field_chunks
            .values()
            .all(|chunk| chunk.codec_name == "plain")
    }) {
        return Err(CassieError::Execution(
            "compressible benchmark fixture selected only plain codecs".to_string(),
        ));
    }
    Ok(())
}

fn incompressible_documents(rows: usize) -> Vec<(Option<String>, serde_json::Value)> {
    (0..rows)
        .map(|index| {
            let mixed = mix(u64::try_from(index).expect("benchmark row should fit u64"));
            (
                Some(format!("incompressible-{index:04}")),
                serde_json::json!({
                    "title": format!("{index:016x}-{mixed:016x}"),
                    "body": format!("{:016x}-{:016x}", mix(mixed), mix(mixed.rotate_left(17))),
                    "score": i64::try_from(mixed % 2_147_483_647).expect("score should fit i64"),
                    "status": format!("{:016x}", mix(mixed.rotate_right(11))),
                    "embedding": [1.0, 0.0, 0.0]
                }),
            )
        })
        .collect()
}

fn compressible_documents(rows: usize) -> Vec<(Option<String>, serde_json::Value)> {
    let bodies = [
        "a".repeat(4_096),
        "b".repeat(4_096),
        "c".repeat(4_096),
        "d".repeat(4_096),
    ];
    let titles = (0..16)
        .map(|position| format!("title-{position:02}-{}", "t".repeat(240)))
        .collect::<Vec<_>>();
    (0..rows)
        .map(|index| {
            (
                Some(format!("doc-{index}")),
                serde_json::json!({
                    "title": titles[index % titles.len()],
                    "body": bodies[index % bodies.len()],
                    "score": i64::try_from(index % 100).expect("score should fit i64"),
                    "status": if index % 2 == 0 { "approved" } else { "pending" },
                    "embedding": [1.0, 0.0, 0.0]
                }),
            )
        })
        .collect()
}

fn alp_documents(rows: usize) -> Vec<(Option<String>, serde_json::Value)> {
    let midpoint = i64::try_from(rows / 2).expect("benchmark rows should fit i64");
    (0..rows)
        .map(|index| {
            let permuted = index
                .checked_mul(977)
                .expect("benchmark permutation should fit usize")
                % rows;
            let position = i64::try_from(permuted).expect("benchmark row should fit i64");
            let centered = i32::try_from(position - midpoint)
                .expect("benchmark centered position should fit i32");
            (
                Some(format!("alp-{index:04}")),
                serde_json::json!({
                    "title": format!("alp-{index:04}"),
                    "body": "alp benchmark row",
                    "score": f64::from(centered) / 100.0,
                    "status": "active",
                    "embedding": [1.0, 0.0, 0.0]
                }),
            )
        })
        .collect()
}

fn fsst_documents(rows: usize) -> Vec<(Option<String>, serde_json::Value)> {
    let common = "account-lifecycle-event-payload/approved/".repeat(8);
    (0..rows)
        .map(|index| {
            let score = usize::from(index.is_multiple_of(256));
            (
                Some(format!("fsst-{index:04}")),
                serde_json::json!({
                    "title": format!("tenant-{:02}-event-{index:04}", index % 64),
                    "body": format!(
                        "tenant-{:02}/event-{index:04}/{common}trace-{:016x}",
                        index % 64,
                        mix(u64::try_from(index).expect("benchmark row should fit u64"))
                    ),
                    "score": i64::try_from(score).expect("benchmark score should fit i64"),
                    "status": "approved",
                    "embedding": [1.0, 0.0, 0.0]
                }),
            )
        })
        .collect()
}

const fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
