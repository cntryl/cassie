use criterion::{criterion_group, criterion_main, Criterion, SamplingMode, Throughput};

#[path = "support/criterion_config.rs"]
mod criterion_config;
#[path = "support/workloads.rs"]
mod workloads;

fn bench_search_vector(c: &mut Criterion) {
    let mut group = c.benchmark_group("tier1_hotpath_search_vector");
    group.sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Elements(1));

    group.bench_function("tokenization", |b| b.iter(workloads::tokenization));
    group.bench_function("bm25_score", |b| b.iter(workloads::bm25_score));
    group.bench_function("cosine_distance", |b| b.iter(workloads::cosine_distance));
    group.bench_function("dot_product", |b| b.iter(workloads::dot_product));
    group.bench_function("l2_distance", |b| b.iter(workloads::l2_distance));
    group.bench_function("hnsw_candidate_search", |b| {
        b.iter(workloads::hnsw_candidate_search)
    });

    group.finish();
}

criterion_group! {
    name = benches;
    config = criterion_config::criterion_config_for_tier1();
    targets = bench_search_vector
}

criterion_main!(benches);
