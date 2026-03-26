use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, Mutex};

use crate::config::Settings;
use crate::daemon::protocol::{ClientMessage, ServerMessage};
use crate::daemon::session::SessionManager;

/// The daemon server that listens on a Unix socket and manages agent sessions.
pub struct DaemonServer {
    sessions: Arc<Mutex<SessionManager>>,
    shutdown_tx: mpsc::Sender<()>,
    shutdown_rx: mpsc::Receiver<()>,
}

impl DaemonServer {
    pub fn new(settings: Settings, max_sessions: usize) -> Self {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        Self {
            sessions: Arc::new(Mutex::new(SessionManager::new(settings, max_sessions))),
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Run the server, listening on the given Unix socket path.
    pub async fn run(mut self, socket_path: &std::path::Path) -> Result<()> {
        // Remove stale socket if it exists
        if socket_path.exists() {
            std::fs::remove_file(socket_path)?;
        }

        // Create parent directory with restricted permissions
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
            }
        }

        let listener = UnixListener::bind(socket_path)?;
        tracing::info!("Daemon listening on {}", socket_path.display());

        // Periodic event drain task
        let sessions_drain = Arc::clone(&self.sessions);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(50));
            loop {
                interval.tick().await;
                sessions_drain.lock().await.drain_all_events();
            }
        });

        loop {
            tokio::select! {
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _addr)) => {
                            let sessions = Arc::clone(&self.sessions);
                            let shutdown_tx = self.shutdown_tx.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_client(stream, sessions, shutdown_tx).await {
                                    tracing::error!("Client handler error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            tracing::error!("Accept error: {}", e);
                        }
                    }
                }
                _ = self.shutdown_rx.recv() => {
                    tracing::info!("Shutdown signal received, stopping server");
                    self.sessions.lock().await.shutdown_all();
                    break;
                }
            }
        }

        // Clean up socket file
        let _ = std::fs::remove_file(socket_path);
        Ok(())
    }
}

/// Handle a single client connection.
async fn handle_client(
    stream: UnixStream,
    sessions: Arc<Mutex<SessionManager>>,
    shutdown_tx: mpsc::Sender<()>,
) -> Result<()> {
    let (reader, writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let writer = Arc::new(Mutex::new(writer));

    // Channel for server messages going back to this client
    let (client_tx, mut client_rx) = mpsc::unbounded_channel::<ServerMessage>();

    // Writer task: forwards messages from client_tx to the socket
    let writer_clone = Arc::clone(&writer);
    let writer_task = tokio::spawn(async move {
        while let Some(msg) = client_rx.recv().await {
            let mut line = serde_json::to_string(&msg).unwrap_or_default();
            line.push('\n');
            let mut w = writer_clone.lock().await;
            if w.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = w.flush().await;
        }
    });

    // Read loop: process incoming ClientMessages
    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        let n = reader.read_line(&mut line_buf).await?;
        if n == 0 {
            // Client disconnected
            break;
        }

        let msg: ClientMessage = match serde_json::from_str(line_buf.trim()) {
            Ok(m) => m,
            Err(e) => {
                let _ = client_tx.send(ServerMessage::Error {
                    message: format!("Invalid message: {}", e),
                });
                continue;
            }
        };

        match msg {
            ClientMessage::Ping => {
                let _ = client_tx.send(ServerMessage::Pong);
            }

            ClientMessage::CreateSession { working_dir } => {
                let mut sm = sessions.lock().await;
                match sm.create_session(working_dir).await {
                    Ok(session_id) => {
                        let _ =
                            client_tx.send(ServerMessage::SessionCreated { session_id });
                    }
                    Err(e) => {
                        let _ = client_tx.send(ServerMessage::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }

            ClientMessage::AttachSession { session_id } => {
                let mut sm = sessions.lock().await;
                if let Some(session) = sm.get_mut(&session_id) {
                    session.subscribe(client_tx.clone());
                    let _ = client_tx.send(ServerMessage::Ok);
                } else {
                    let _ = client_tx.send(ServerMessage::Error {
                        message: format!("Session not found: {}", session_id),
                    });
                }
            }

            ClientMessage::DetachSession { session_id } => {
                let mut sm = sessions.lock().await;
                if let Some(session) = sm.get_mut(&session_id) {
                    session.remove_subscriber(&client_tx);
                    let _ = client_tx.send(ServerMessage::Ok);
                } else {
                    let _ = client_tx.send(ServerMessage::Error {
                        message: format!("Session not found: {}", session_id),
                    });
                }
            }

            ClientMessage::ListSessions => {
                let sm = sessions.lock().await;
                let _ = client_tx.send(ServerMessage::SessionList {
                    sessions: sm.list(),
                });
            }

            ClientMessage::SendMessage { session_id, text } => {
                let mut sm = sessions.lock().await;
                if let Some(session) = sm.get_mut(&session_id) {
                    session.send_message(text);
                    let _ = client_tx.send(ServerMessage::Ok);
                } else {
                    let _ = client_tx.send(ServerMessage::Error {
                        message: format!("Session not found: {}", session_id),
                    });
                }
            }

            ClientMessage::ApproveAction {
                session_id,
                approved,
            } => {
                let mut sm = sessions.lock().await;
                if let Some(session) = sm.get_mut(&session_id) {
                    if approved {
                        // The approval execution happens via the agent handle
                        // For now, acknowledge and let the client drive it
                        let _ = client_tx.send(ServerMessage::Ok);
                    } else {
                        session.agent.add_tool_result(
                            "unknown".to_string(),
                            "DENIED: User rejected this action.".to_string(),
                        );
                        let _ = client_tx.send(ServerMessage::Ok);
                    }
                } else {
                    let _ = client_tx.send(ServerMessage::Error {
                        message: format!("Session not found: {}", session_id),
                    });
                }
            }

            ClientMessage::CancelTurn { session_id } => {
                // TODO: implement cancellation via CancellationToken
                let _ = client_tx.send(ServerMessage::Ok);
                tracing::warn!(
                    "CancelTurn for session {} not fully implemented yet",
                    session_id
                );
            }

            ClientMessage::CompactContext { session_id } => {
                let _ = client_tx.send(ServerMessage::Ok);
                tracing::info!("CompactContext for session {}", session_id);
            }

            ClientMessage::Shutdown => {
                let _ = client_tx.send(ServerMessage::Ok);
                let _ = shutdown_tx.send(()).await;
            }
        }
    }

    // Clean up: remove this client's subscriber from all sessions
    // (The subscriber list auto-prunes on failed sends, but let's be explicit.)
    writer_task.abort();

    Ok(())
}
