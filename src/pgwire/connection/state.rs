use super::{CassieSession, ReadyState};
use crate::pgwire::protocol::{Portal, PreparedStatement};
use crate::runtime::{QueryExecutionControls, RuntimeState};
use std::collections::HashMap;

#[derive(Debug)]
pub(super) enum HandshakeState {
    AwaitStartup,
    AwaitPassword {
        user: String,
        database: Option<String>,
    },
    Ready,
}

#[derive(Debug)]
pub(super) enum StartupFrame {
    SslRequest,
    CancelRequest { process_id: i32, secret_key: i32 },
    Startup(HashMap<String, String>),
}

#[derive(Debug)]
pub(super) enum HandshakeError {
    Closed,
    Invalid(String),
}

#[derive(Debug)]
pub(super) enum FrontendMessage {
    Parse {
        name: String,
        query: String,
        parameter_type_oids: Vec<i32>,
    },
    Bind {
        portal: String,
        statement: String,
        parameter_formats: Vec<i16>,
        parameters: Vec<Option<Vec<u8>>>,
        result_formats: Vec<i16>,
    },
    Describe {
        target: DescribeTarget,
        name: String,
    },
    Execute {
        portal: String,
        max_rows: i32,
    },
    Close {
        target: DescribeTarget,
        name: String,
    },
    CopyData(Vec<u8>),
    CopyDone,
    CopyFail(String),
    FunctionCall,
    Sync,
    Flush,
    Terminate,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DescribeTarget {
    Statement,
    Portal,
}

#[derive(Debug)]
pub(super) struct SessionState {
    pub(super) session: Option<CassieSession>,
    pub(super) startup_user: Option<String>,
    pub(super) startup_database: Option<String>,
    pub(super) authenticated: bool,
    pub(super) ready: ReadyState,
    pub(super) prepared_statements: HashMap<String, PreparedStatement>,
    pub(super) portals: HashMap<String, Portal>,
    pub(super) portal_cursors: HashMap<String, crate::midge::adapter::MidgeRowCursor>,
    pub(super) portal_memory_controls: QueryExecutionControls,
    pub(super) next_prepared_id: u64,
    pub(super) backend_registration: Option<crate::runtime::PgwireBackendRegistration>,
}

impl SessionState {
    pub(super) fn new(limits: &crate::config::CassieRuntimeLimits) -> Self {
        Self {
            session: None,
            startup_user: None,
            startup_database: None,
            authenticated: false,
            ready: ReadyState::InTransaction,
            prepared_statements: HashMap::new(),
            portals: HashMap::new(),
            portal_cursors: HashMap::new(),
            portal_memory_controls: QueryExecutionControls::from_limits(
                limits,
                std::time::Instant::now(),
            ),
            next_prepared_id: 1,
            backend_registration: None,
        }
    }

    pub(super) fn next_prepared_id(&mut self) -> u64 {
        let id = self.next_prepared_id;
        self.next_prepared_id = self.next_prepared_id.saturating_add(1);
        id
    }

    pub(super) fn clear_portal_execution(&mut self, name: &str) {
        let cancellation = self
            .portals
            .get_mut(name)
            .and_then(|portal| portal.suspended.take())
            .and_then(|suspended| suspended.cancellation);
        self.portal_cursors.remove(name);
        if let (Some(registration), Some(cancellation)) =
            (self.backend_registration.as_ref(), cancellation.as_ref())
        {
            registration.clear_query(cancellation);
        }
    }

    pub(super) fn remove_portal(&mut self, name: &str) -> Option<Portal> {
        self.clear_portal_execution(name);
        self.portals.remove(name)
    }

    pub(super) fn remove_portals_for_prepared_id(&mut self, prepared_id: u64) -> usize {
        let names = self
            .portals
            .iter()
            .filter_map(|(name, portal)| {
                (portal.prepared_id == prepared_id).then_some(name.clone())
            })
            .collect::<Vec<_>>();
        for name in &names {
            self.remove_portal(name);
        }
        names.len()
    }

    pub(super) fn clear_query_cancellation(
        &self,
        cancellation: &crate::runtime::QueryCancellationHandle,
    ) {
        if let Some(registration) = self.backend_registration.as_ref() {
            registration.clear_query(cancellation);
        }
    }

    pub(super) fn cleanup_pgwire_objects(&mut self, runtime: &RuntimeState) {
        record_negative_delta(self.prepared_statements.len(), |delta| {
            runtime.record_pgwire_prepared_delta(delta);
        });
        record_negative_delta(self.portals.len(), |delta| {
            runtime.record_pgwire_portal_delta(delta);
        });
        self.prepared_statements.clear();
        let portal_names = self.portals.keys().cloned().collect::<Vec<_>>();
        for name in portal_names {
            self.remove_portal(&name);
        }
    }
}

fn record_negative_delta(count: usize, record: impl FnOnce(isize)) {
    if count == 0 {
        return;
    }
    let delta = isize::try_from(count).unwrap_or(isize::MAX);
    record(-delta);
}
