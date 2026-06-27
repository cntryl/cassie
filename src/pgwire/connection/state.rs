use super::*;

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
    CancelRequest,
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
        parameter_types: Vec<i32>,
    },
    Bind {
        portal_name: String,
        statement_name: String,
        parameter_formats: Vec<i16>,
        params: Vec<Option<Vec<u8>>>,
        result_formats: Vec<i16>,
    },
    Describe {
        target: DescribeTarget,
        name: String,
    },
    Execute {
        portal_name: String,
        limit: Option<i64>,
    },
    Close {
        target: CloseTarget,
        name: String,
    },
    CopyData(Vec<u8>),
    CopyDone,
    CopyFail(String),
    FunctionCall,
    Sync,
    Flush,
    Terminate,
    Unknown(u8),
}

#[derive(Debug)]
pub(super) enum DescribeTarget {
    Statement,
    Portal,
}

#[derive(Debug)]
pub(super) enum CloseTarget {
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
    pub(super) next_prepared_id: u64,
    pub(super) prepared: HashMap<String, PreparedStatement>,
    pub(super) portals: HashMap<String, Portal>,
}

impl SessionState {
    pub(super) fn new() -> Self {
        Self {
            session: None,
            startup_user: None,
            startup_database: None,
            authenticated: false,
            ready: ReadyState::InTransaction,
            next_prepared_id: 1,
            prepared: HashMap::new(),
            portals: HashMap::new(),
        }
    }
}
