use crate::pgwire::protocol::ClientMessage;

#[must_use]
pub fn decode_startup(msg: &ClientMessage) -> (String, Option<String>) {
    match msg {
        ClientMessage::Startup { user, database } => (user.clone(), database.clone()),
        _ => ("postgres".to_string(), None),
    }
}
