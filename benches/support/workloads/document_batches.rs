pub const BENCH_DOCUMENT_WRITE_BATCH_ROWS: usize = 5_000;

pub fn bench_document_write_batch_ranges(
    dataset_rows: usize,
) -> impl Iterator<Item = std::ops::Range<usize>> {
    (0..dataset_rows)
        .step_by(BENCH_DOCUMENT_WRITE_BATCH_ROWS)
        .map(move |start| {
            start
                ..start
                    .saturating_add(BENCH_DOCUMENT_WRITE_BATCH_ROWS)
                    .min(dataset_rows)
        })
}
