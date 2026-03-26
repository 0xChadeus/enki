use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use crate::daemon::lifecycle;
use crate::daemon::protocol::{ClientMessage, ServerMessage};

/// Client that connects to a running Enki daemon over a Unix socket.
pub struct DaemonClient {
    writer: tokio::io::WriteHalf<UnixStream>,
    /// Receiver for messages from the daemon.
    msg_rx: mpsc::UnboundedReceiver<ServerMessage>,
}

impl DaemonClient {
    /// Connect to the daemon's Unix socket.
    pub async fn connect() -> Result<Self> {
        let path = lifecycle::socket_path();
        let stream = UnixStream::connect(&path)
            .await
            .with_context(|| format!("Cannot connect to daemon at {}", path.display()))?;

        let (reader, writer) = tokio::io::split(stream);

        let (msg_tx, msg_rx) = mpsc::unbounded_channel();

        // Spawn a reader task that parses newline-delimited JSON
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Ok(msg) = serde_json::from_str::<ServerMessage>(line.trim()) {
                            if msg_tx.send(msg).is_err() {
                                break; // receiver dropped
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self { writer, msg_rx })
    }

    /// Send a message to the daemon.
    pub async fn send(&mut self, msg: &ClientMessage) -> Result<()> {
        let mut line = serde_json::to_string(msg)?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Receive the next message from the daemon (blocking).
    pub async fn recv(&mut self) -> Option<ServerMessage> {
        self.msg_rx.recv().await
    }

    /// Try to receive a message without blocking.
    pub fn try_recv(&mut self) -> Option<ServerMessage> {
        self.msg_rx.try_recv().ok()
    }

    /// Check if the daemon is running by attempting to connect and ping.
    pub async fn is_daemon_running() -> bool {
        if !lifecycle::is_running() {
            return false;
        }
        // Double-check by trying to connect
        match Self::connect().await {
            Ok(mut client) => {
                if client.send(&ClientMessage::Ping).await.is_ok() {
                    // Try to read pong with a timeout
                    let result = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        client.recv(),
                    )
                    .await;
                    matches!(result, Ok(Some(ServerMessage::Pong)))
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }
}
