use std::future::{ready, Ready};

use cassie::app::CassieError;
use cassie::catalog::canonical_relation_name;

use super::context::{bench_document_schema, unindexed_context, BenchContext};

pub const COMPRESSIBLE_AUTO_SQL: &str =
    "SELECT title, body FROM bench_documents WHERE status = 'approved' AND score >= 90";
pub const COMPRESSIBLE_PLAIN_SQL: &str =
    "SELECT title, body FROM bench_documents_plain WHERE status = 'approved' AND score >= 90";
pub const INCOMPRESSIBLE_AUTO_SQL: &str =
    "SELECT title, body FROM bench_documents_incompressible WHERE title >= '000000000000079c-'";
pub const INCOMPRESSIBLE_PLAIN_SQL: &str =
    "SELECT title, body FROM bench_documents_incompressible_plain WHERE title >= '000000000000079c-'";

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

    Ok(context)
}

fn create_bench_collection(
    context: &BenchContext,
    collection: &str,
    documents: Vec<(Option<String>, serde_json::Value)>,
) -> Result<(), CassieError> {
    let schema = bench_document_schema();
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

const fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}
