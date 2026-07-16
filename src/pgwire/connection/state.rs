use super::{CassieSession, ReadyState};
use crate::pgwire::protocol::{Portal, PreparedStatement};
use crate::runtime::RuntimeState;
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
    pub(super) next_prepared_id: u64,
    pub(super) backend_registration: Option<crate::runtime::PgwireBackendRegistration>,
}

impl SessionState {
    pub(super) fn new() -> Self {
        Self {
            session: None,
            startup_user: None,
            startup_database: None,
            authenticated: false,
            ready: ReadyState::InTransaction,
            prepared_statements: HashMap::new(),
            portals: HashMap::new(),
            portal_cursors: HashMap::new(),
            next_prepared_id: 1,
            backend_registration: None,
        }
    }

    pub(super) fn next_prepared_id(&mut self) -> u64 {
        let id = self.next_prepared_id;
        self.next_prepared_id = self.next_prepared_id.saturating_add(1);
        id
    }

    pub(super) fn cleanup_pgwire_objects(&mut self, runtime: &RuntimeState) {
        record_negative_delta(self.prepared_statements.len(), |delta| {
            runtime.record_pgwire_prepared_delta(delta);
        });
        record_negative_delta(self.portals.len(), |delta| {
            runtime.record_pgwire_portal_delta(delta);
        });
        self.prepared_statements.clear();
        self.portals.clear();
        self.portal_cursors.clear();
    }
}

fn record_negative_delta(count: usize, record: impl FnOnce(isize)) {
    if count == 0 {
        return;
    }
    let delta = isize::try_from(count).unwrap_or(isize::MAX);
    record(-delta);
}
