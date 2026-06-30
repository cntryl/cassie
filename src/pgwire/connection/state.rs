use super::{CassieSession, ReadyState};
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
    Parse,
    Bind,
    Describe,
    Execute,
    Close,
    CopyData(Vec<u8>),
    CopyDone,
    CopyFail(String),
    FunctionCall,
    Sync,
    Flush,
    Terminate,
    Unknown,
}

#[derive(Debug)]
pub(super) struct SessionState {
    pub(super) session: Option<CassieSession>,
    pub(super) startup_user: Option<String>,
    pub(super) startup_database: Option<String>,
    pub(super) authenticated: bool,
    pub(super) ready: ReadyState,
}

impl SessionState {
    pub(super) fn new() -> Self {
        Self {
            session: None,
            startup_user: None,
            startup_database: None,
            authenticated: false,
            ready: ReadyState::InTransaction,
        }
    }
}
