use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use tokio::sync::mpsc;

use crate::agent::handle::AgentHandle;
use crate::agent::r#loop::AgentEvent;
use crate::config::Settings;
use crate::daemon::protocol::{ServerMessage, SessionInfo};

/// A single agent session managed by the daemon.
pub struct Session {
    pub session_id: String,
    pub working_dir: PathBuf,
    pub model_name: String,
    pub created_at: String,
    pub agent: AgentHandle,
    /// Senders for clients currently subscribed to this session's events.
    subscribers: Vec<mpsc::UnboundedSender<ServerMessage>>,
    /// Receiver for agent events (from the current turn).
    event_rx: Option<mpsc::UnboundedReceiver<AgentEvent>>,
}

impl Session {
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.session_id.clone(),
            working_dir: self.working_dir.clone(),
            model_name: self.model_name.clone(),
            created_at: self.created_at.clone(),
        }
    }

    /// Subscribe a client to receive events from this session.
    pub fn subscribe(&mut self, tx: mpsc::UnboundedSender<ServerMessage>) {
        self.subscribers.push(tx);
    }

    /// Remove disconnected subscribers.
    pub fn remove_subscriber(&mut self, tx: &mpsc::UnboundedSender<ServerMessage>) {
        self.subscribers.retain(|s| !s.same_channel(tx));
    }

    /// Broadcast an agent event to all subscribers.
    fn broadcast(&mut self, event: AgentEvent) {
        let msg = ServerMessage::Event {
            session_id: self.session_id.clone(),
            event,
        };
        self.subscribers.retain(|tx| tx.send(msg.clone()).is_ok());
    }

    /// Send a message to the agent; events will be forwarded to subscribers.
    pub fn send_message(&mut self, text: String) {
        let rx = self.agent.send_message(text);
        self.event_rx = Some(rx);
    }

    /// Drain any available agent events and broadcast to subscribers.
    /// Returns true if the event stream is still active.
    pub fn drain_events(&mut self) -> bool {
        let rx = match self.event_rx.as_mut() {
            Some(rx) => rx,
            None => return false,
        };

        let mut events = Vec::new();
        loop {
            match rx.try_recv() {
                Ok(event) => events.push(event),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    self.event_rx = None;
                    break;
                }
            }
        }

        let mut stream_ended = false;
        for event in events {
            if matches!(event, AgentEvent::TurnComplete) {
                stream_ended = true;
            }
            self.broadcast(event);
        }

        if stream_ended {
            self.event_rx = None;
            return false;
        }

        self.event_rx.is_some()
    }
}

/// Manages all active sessions.
pub struct SessionManager {
    sessions: HashMap<String, Session>,
    settings: Settings,
    max_sessions: usize,
}

impl SessionManager {
    pub fn new(settings: Settings, max_sessions: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            settings,
            max_sessions,
        }
    }

    /// Create a new session for the given working directory.
    pub async fn create_session(&mut self, working_dir: PathBuf) -> Result<String> {
        if self.sessions.len() >= self.max_sessions {
            anyhow::bail!(
                "Maximum number of sessions ({}) reached",
                self.max_sessions
            );
        }

        let agent = AgentHandle::spawn(self.settings.clone(), working_dir.clone()).await?;
        let session_id = uuid::Uuid::new_v4().to_string();
        let model_name = agent.model_name().to_string();

        let session = Session {
            session_id: session_id.clone(),
            working_dir,
            model_name,
            created_at: Utc::now().to_rfc3339(),
            agent,
            subscribers: Vec::new(),
            event_rx: None,
        };

        self.sessions.insert(session_id.clone(), session);
        Ok(session_id)
    }

    pub fn get(&self, session_id: &str) -> Option<&Session> {
        self.sessions.get(session_id)
    }

    pub fn get_mut(&mut self, session_id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(session_id)
    }

    pub fn remove(&mut self, session_id: &str) -> Option<Session> {
        self.sessions.remove(session_id)
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions.values().map(|s| s.info()).collect()
    }

    /// Drain events from all active sessions and broadcast to subscribers.
    pub fn drain_all_events(&mut self) {
        for session in self.sessions.values_mut() {
            session.drain_events();
        }
    }

    /// Shut down all sessions.
    pub fn shutdown_all(&mut self) {
        for (_, session) in self.sessions.drain() {
            session.agent.shutdown();
        }
    }
}
