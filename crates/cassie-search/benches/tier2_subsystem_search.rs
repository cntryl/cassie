use std::time::Instant;

use cassie_bench_support::stress::{BenchmarkObservation, CountedObservation};
use cassie_bench_support::{performance_benchmarks, stress};
use cassie_search::inverted_index::InvertedIndex;

const DOCUMENTS: usize = 2_048;
const INVOCATIONS_PER_SAMPLE: usize = 512;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_search",
    );
    let case = stress::StressCase::new("posting_merge", "2k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            DOCUMENTS,
            "tier2_subsystem_search/2k",
        ),
        stress::OperationUnit::Candidate,
    );
    if runner.is_enabled(&case) {
        let setup_started = Instant::now();
        let fixture = PostingMergeFixture::new(DOCUMENTS);
        runner.measure_counted(
            case.parameter(
                "fixture_invocations_per_sample",
                INVOCATIONS_PER_SAMPLE.to_string(),
            )
            .metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().max(1).to_string(),
            ),
            || fixture.merge_batch(INVOCATIONS_PER_SAMPLE),
        );
    }
    runner.finish();
}

struct PostingMergeFixture {
    index: InvertedIndex,
    query_tokens: [String; 2],
}

impl PostingMergeFixture {
    fn new(rows: usize) -> Self {
        let mut index = InvertedIndex::default();
        for row in 0..rows {
            let tokens = if row % 2 == 0 {
                ["alpha".to_owned(), "common".to_owned()]
            } else {
                ["beta".to_owned(), "common".to_owned()]
            };
            index.index_document(&format!("doc-{row}"), &tokens);
        }
        Self {
            index,
            query_tokens: ["alpha".to_owned(), "beta".to_owned()],
        }
    }

    // Isolated in-memory posting-union baseline; Tier 5 measures database full-text retrieval.
    fn merge_batch(&self, invocations: usize) -> CountedBatch {
        let mut result_count = 0_u64;
        for _ in 0..invocations {
            let candidates = self.index.candidate_documents(&self.query_tokens);
            assert_eq!(candidates.len(), DOCUMENTS);
            result_count = result_count
                .checked_add(u64::try_from(candidates.len()).expect("candidate count fits u64"))
                .expect("candidate total fits u64");
            std::hint::black_box(candidates);
        }
        CountedBatch::new(result_count, result_count, Some(result_count))
    }
}

struct CountedBatch {
    completed_operations: u64,
    result_cardinality: u64,
    candidate_count: Option<u64>,
}

impl CountedBatch {
    const fn new(
        completed_operations: u64,
        result_cardinality: u64,
        candidate_count: Option<u64>,
    ) -> Self {
        Self {
            completed_operations,
            result_cardinality,
            candidate_count,
        }
    }
}

impl BenchmarkObservation for CountedBatch {
    fn cardinality(&self) -> u64 {
        self.result_cardinality
    }

    fn candidate_count(&self) -> Option<u64> {
        self.candidate_count
    }
}

impl CountedObservation for CountedBatch {
    fn completed_operations(&self) -> u64 {
        self.completed_operations
    }

    fn result_cardinality(&self) -> u64 {
        self.result_cardinality
    }

    fn candidate_count(&self) -> Option<u64> {
        self.candidate_count
    }

    fn peak_query_memory_bytes(&self) -> Option<u64> {
        None
    }

    fn finish_sample(self) {}
}
