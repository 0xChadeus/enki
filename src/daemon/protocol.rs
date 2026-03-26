use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::agent::r#loop::AgentEvent;

// ---------------------------------------------------------------------------
// Client → Daemon
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMessage {
    /// Create a new agent session for a working directory.
    CreateSession { working_dir: PathBuf },
    /// Attach to an existing session's event stream.
    AttachSession { session_id: String },
    /// Detach from a session's event stream (stops receiving events).
    DetachSession { session_id: String },
    /// List all active sessions.
    ListSessions,
    /// Send a user message to an agent session.
    SendMessage { session_id: String, text: String },
    /// Respond to a pending tool-approval request.
    ApproveAction { session_id: String, approved: bool },
    /// Cancel the current agent turn.
    CancelTurn { session_id: String },
    /// Trigger context compaction on a session.
    CompactContext { session_id: String },
    /// Ask the daemon to shut down gracefully.
    Shutdown,
    /// Ping (health check).
    Ping,
}

// ---------------------------------------------------------------------------
// Daemon → Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    /// A session was created.
    SessionCreated { session_id: String },
    /// List of active sessions.
    SessionList { sessions: Vec<SessionInfo> },
    /// An agent event forwarded from a session.
    Event {
        session_id: String,
        event: AgentEvent,
    },
    /// Generic success acknowledgement.
    Ok,
    /// An error occurred.
    Error { message: String },
    /// Response to Ping.
    Pong,
}

/// Summary information about an active session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub working_dir: PathBuf,
    pub model_name: String,
    pub created_at: String,
}
