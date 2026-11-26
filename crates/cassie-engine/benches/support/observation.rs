use cassie_bench_support::stress::{BenchmarkObservation, CountedObservation};

pub struct ObservationBridge(pub cassie_engine::benchmark::KernelObservation);

impl BenchmarkObservation for ObservationBridge {
    fn cardinality(&self) -> u64 {
        self.0.result_cardinality()
    }

    fn candidate_count(&self) -> Option<u64> {
        self.0.candidate_count()
    }

    fn peak_query_memory_bytes(&self) -> Option<u64> {
        self.0.peak_query_memory_bytes()
    }
}

impl CountedObservation for ObservationBridge {
    fn completed_operations(&self) -> u64 {
        self.0.completed_operations()
    }

    fn result_cardinality(&self) -> u64 {
        self.0.result_cardinality()
    }

    fn candidate_count(&self) -> Option<u64> {
        self.0.candidate_count()
    }

    fn peak_query_memory_bytes(&self) -> Option<u64> {
        self.0.peak_query_memory_bytes()
    }

    fn finish_sample(self) {
        self.0.finish_sample();
    }
}
