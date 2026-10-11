//! Ephemeral phase evidence; no engine or session owner is retained here.
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Instant;

use cassie::midge::Midge;

#[derive(Clone, Copy)]
pub(super) enum RetrievalPhase {
    ThreadLocalGuard,
    StorageConfiguration,
    EngineConstruction,
    Startup,
    SetupSession,
    CreateTable,
    SeedRows,
    BuildFulltext,
    BuildHnsw,
    BuildIvfFlat,
    ReaderCreation,
    QueryCreation,
    SelectedFirst,
    SelectedSecond,
    DropFulltext,
    DropHnsw,
    DropIvfFlat,
    ExactFallback,
    Assertions,
    QueryLocalsRetirement,
    FixtureFieldsRetirement,
    DirectoryCleanup,
    EscapedReaderResults,
    ServerTeardown,
}

impl RetrievalPhase {
    const fn label(self) -> &'static str {
        match self {
            Self::ThreadLocalGuard => "thread_local_guard",
            Self::StorageConfiguration => "storage_configuration",
            Self::EngineConstruction => "engine_construction",
            Self::Startup => "startup",
            Self::SetupSession => "setup_session",
            Self::CreateTable => "create_table",
            Self::SeedRows => "seed_rows",
            Self::BuildFulltext => "build_fulltext",
            Self::BuildHnsw => "build_hnsw",
            Self::BuildIvfFlat => "build_ivfflat",
            Self::ReaderCreation => "reader_creation",
            Self::QueryCreation => "query_creation",
            Self::SelectedFirst => "selected_first",
            Self::SelectedSecond => "selected_second",
            Self::DropFulltext => "drop_fulltext",
            Self::DropHnsw => "drop_hnsw",
            Self::DropIvfFlat => "drop_ivfflat",
            Self::ExactFallback => "exact_fallback",
            Self::Assertions => "assertions",
            Self::QueryLocalsRetirement => "query_locals_retirement",
            Self::FixtureFieldsRetirement => "fixture_fields_retirement",
            Self::DirectoryCleanup => "directory_cleanup",
            Self::EscapedReaderResults => "escaped_reader_results",
            Self::ServerTeardown => "server_teardown",
        }
    }
}

#[derive(Clone)]
struct EvidenceContext {
    invocation: uuid::Uuid,
    caller: &'static str,
    case: &'static str,
    path: Option<String>,
    started: Instant,
    sequence: Arc<AtomicU64>,
    midge: Option<Weak<Midge>>,
}

#[derive(Clone, Default)]
pub(super) struct RetrievalPhaseEvidence {
    context: Option<EvidenceContext>,
}

impl RetrievalPhaseEvidence {
    pub(super) fn new(caller: &'static str) -> Self {
        Self {
            context: Some(EvidenceContext {
                invocation: uuid::Uuid::new_v4(),
                caller,
                case: "-",
                path: None,
                started: Instant::now(),
                sequence: Arc::new(AtomicU64::new(0)),
                midge: None,
            }),
        }
    }

    pub(super) fn for_case(&self, case: &'static str) -> Self {
        let mut evidence = self.clone();
        if let Some(context) = &mut evidence.context {
            context.case = case;
        }
        evidence
    }

    pub(super) fn for_path(&self, path: &str) -> Self {
        let mut evidence = self.clone();
        if let Some(context) = &mut evidence.context {
            context.path = Some(path.to_owned());
        }
        evidence
    }

    pub(super) fn observe_midge(&mut self, midge: &Arc<Midge>) {
        if let Some(context) = &mut self.context {
            context.midge = Some(Arc::downgrade(midge));
        }
    }

    pub(super) fn entered(&self, phase: RetrievalPhase) {
        self.record(phase, "entered");
    }

    pub(super) fn returned(&self, phase: RetrievalPhase) {
        self.record(phase, "returned");
    }

    pub(super) fn not_applicable(&self, phase: RetrievalPhase) {
        self.record(phase, "not_applicable");
    }

    pub(super) fn query_locals_scope(&self) -> QueryLocalsScope {
        QueryLocalsScope(self.clone())
    }

    fn record(&self, phase: RetrievalPhase, state: &str) {
        let Some(context) = &self.context else {
            return;
        };
        // One short raw stderr lock per record; never held across an operation.
        let mut stderr = std::io::stderr().lock();
        let sequence = context.sequence.fetch_add(1, Ordering::Relaxed);
        let strong_owners = context.midge.as_ref().map(Weak::strong_count);
        let _ = writeln!(
            stderr,
            "CASSIE_RETRIEVAL_PHASE pid={} invocation={} caller={} case={} path={} \
             seq={sequence} phase={} state={state} elapsed_us={} \
             midge_strong_owners={strong_owners:?} unwinding={}",
            std::process::id(),
            context.invocation,
            context.caller,
            context.case,
            context.path.as_deref().unwrap_or("-"),
            phase.label(),
            context.started.elapsed().as_micros(),
            std::thread::panicking(),
        );
        let _ = stderr.flush();
    }
}

/// Metadata-only sentinel declared before the existing query/session locals.
pub(super) struct QueryLocalsScope(RetrievalPhaseEvidence);

impl Drop for QueryLocalsScope {
    fn drop(&mut self) {
        self.0.returned(RetrievalPhase::QueryLocalsRetirement);
    }
}
