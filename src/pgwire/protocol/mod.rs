#[derive(Debug, Clone)]
pub enum ClientMessage {
    Startup {
        user: String,
        database: Option<String>,
    },
    Password {
        user: String,
        password: String,
    },
    Query(String),
    Parse {
        name: String,
        query: String,
    },
    Bind {
        name: String,
        params: Vec<String>,
    },
    Describe(String),
    Execute {
        name: String,
        limit: Option<i64>,
    },
    Sync,
    Close(String),
    Unknown(String),
}

#[derive(Debug)]
pub enum ServerMessage {
    AuthenticationOk,
    AuthChallenge,
    ParseComplete,
    BindComplete,
    CloseComplete,
    RowDescription(Vec<String>),
    DataRow(Vec<String>),
    CommandComplete(String),
    ReadyForQuery,
    ErrorResponse(String),
    SyncComplete,
}

#[derive(Debug, Clone)]
pub enum WireError {
    ParseError(String),
    BindError(String),
    NotAuthenticated,
    Unsupported(String),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WireError::ParseError(message) => write!(f, "parse error: {message}"),
            WireError::BindError(message) => write!(f, "bind error: {message}"),
            WireError::NotAuthenticated => write!(f, "not authenticated"),
            WireError::Unsupported(message) => write!(f, "unsupported: {message}"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ReadyState {
    Idle,
    InTransaction,
}

#[derive(Debug, Clone)]
pub struct PreparedStatement {
    pub name: String,
    pub query: String,
}

#[derive(Debug, Clone)]
pub struct Portal {
    pub name: String,
    pub statement_name: String,
    pub limit: Option<i64>,
    pub params: Vec<String>,
}

impl ServerMessage {
    pub fn as_wire(&self) -> String {
        match self {
            ServerMessage::AuthenticationOk => "OK auth".to_string(),
            ServerMessage::AuthChallenge => "AUTH".to_string(),
            ServerMessage::ParseComplete => "PARSE_OK".to_string(),
            ServerMessage::BindComplete => "BIND_OK".to_string(),
            ServerMessage::CloseComplete => "CLOSE_OK".to_string(),
            ServerMessage::RowDescription(cols) => format!("ROWDESC {}", cols.join(",")),
            ServerMessage::DataRow(values) => format!("DATAROW {}", values.join("\t")),
            ServerMessage::CommandComplete(msg) => format!("DONE {}", msg),
            ServerMessage::ReadyForQuery => "READY_FOR_QUERY".to_string(),
            ServerMessage::ErrorResponse(msg) => format!("ERR {}", msg),
            ServerMessage::SyncComplete => "SYNC".to_string(),
        }
    }
}

pub fn encode(message: &ServerMessage) -> Vec<u8> {
    let mut out = Vec::new();
    let text = message.as_wire();
    out.extend_from_slice(text.as_bytes());
    out.push(b'\n');
    out
}

pub fn decode(line: &str) -> ClientMessage {
    let trimmed = line.trim_end();
    if trimmed.starts_with("STARTUP") {
        let mut user = "postgres".to_string();
        let mut database = None;
        let parts: Vec<_> = trimmed.split_whitespace().collect();
        for part in parts.iter().skip(1) {
            if let Some((k, v)) = part.split_once('=') {
                match k.to_lowercase().as_str() {
                    "user" => user = v.to_string(),
                    "database" => database = Some(v.to_string()),
                    _ => {}
                }
            }
        }
        return ClientMessage::Startup { user, database };
    }
    if trimmed.starts_with("PASSWORD") {
        let parts: Vec<_> = trimmed.split_whitespace().collect();
        let mut user = "postgres".to_string();
        let mut password = String::new();

        for part in parts.iter().skip(1) {
            if let Some((key, value)) = part.split_once('=') {
                match key.to_lowercase().as_str() {
                    "user" => user = value.to_string(),
                    "password" => password = value.to_string(),
                    _ => {}
                }
            } else if parts.len() > 1 && password.is_empty() {
                password = part.to_string();
            }
        }
        return ClientMessage::Password { user, password };
    }
    if trimmed.starts_with("PARSE") {
        let rest = trimmed.trim_start_matches("PARSE").trim();
        let (name, query) = if let Some((left, right)) = rest.split_once('|') {
            (left.to_string(), right.to_string())
        } else {
            let mut parts = rest.splitn(2, char::is_whitespace);
            let left = parts.next().unwrap_or("_pstmt_").trim().to_string();
            let right = parts.next().unwrap_or("").trim().to_string();
            if right.is_empty() {
                ("_pstmt_".to_string(), left)
            } else {
                (left, right)
            }
        };
        return ClientMessage::Parse { name, query };
    }
    if trimmed.starts_with("BIND") {
        let rest = trimmed.trim_start_matches("BIND").trim();
        let (name, params) = if let Some((head, tail)) = rest.split_once('|') {
            let mut values = Vec::new();
            let mut head_parts = head.split_whitespace();
            let name = head_parts.next().unwrap_or("_pstmt_").to_string();
            if let Some(first) = head_parts.next() {
                values.push(first.to_string());
            }
            for value in tail.split('|') {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    values.push(trimmed.to_string());
                }
            }
            (name, values)
        } else {
            let mut split = rest.split_whitespace();
            let name = split.next().unwrap_or("_pstmt_").to_string();
            let params = split.map(|value| value.to_string()).collect();
            (name, params)
        };
        let name = if name.is_empty() {
            "_pstmt_".to_string()
        } else {
            name
        };
        let params = if params.is_empty() {
            Vec::new()
        } else {
            params
        };
        return ClientMessage::Bind { name, params };
    }
    if trimmed.starts_with("DESCRIBE") {
        let rest = trimmed.trim_start_matches("DESCRIBE").trim();
        let name = rest.split_whitespace().next().unwrap_or("_pstmt_");
        return ClientMessage::Describe(name.to_string());
    }
    if trimmed.starts_with("EXECUTE") {
        let rest = trimmed.trim_start_matches("EXECUTE").trim();
        let mut parts = rest.split_whitespace();
        let name = parts.next().unwrap_or("_pstmt_").to_string();
        let limit = parts.next().and_then(|raw| raw.parse::<i64>().ok());
        return ClientMessage::Execute { name, limit };
    }
    if trimmed.starts_with("CLOSE") {
        let name = trimmed
            .trim_start_matches("CLOSE")
            .split_whitespace()
            .next()
            .unwrap_or("_pstmt_");
        return ClientMessage::Close(name.to_string());
    }
    if trimmed == "SYNC" {
        return ClientMessage::Sync;
    }
    if trimmed.starts_with("QUERY") {
        return ClientMessage::Query(trimmed.trim_start_matches("QUERY").trim().to_string());
    }
    ClientMessage::Unknown(trimmed.to_string())
}
