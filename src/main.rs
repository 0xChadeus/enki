#![allow(dead_code, unused_imports)]

mod agent;
mod app;
mod config;
mod daemon;
mod llm;
mod tools;
mod tui;

use std::io;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;

use crate::config::Settings;

#[derive(Parser)]
#[command(name = "enki", version, about = "Enki — Local AI coding assistant powered by Ollama")]
struct Cli {
    /// Working directory (defaults to current directory)
    #[arg(short = 'C', long, global = true)]
    directory: Option<PathBuf>,

    /// Ollama model to use
    #[arg(short, long, global = true)]
    model: Option<String>,

    /// Ollama server URL
    #[arg(long, global = true)]
    url: Option<String>,

    /// Force standalone mode (don't connect to daemon)
    #[arg(long)]
    standalone: bool,

    /// Force connection to a running daemon (error if not running)
    #[arg(long, conflicts_with = "standalone")]
    connect: bool,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Manage the background daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Send a one-shot message to the daemon and print the response
    Send {
        /// The message to send
        message: String,
    },
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon in the background
    Start {
        /// Run in the foreground (don't daemonize; useful for systemd or debugging)
        #[arg(long)]
        foreground: bool,
    },
    /// Stop a running daemon
    Stop,
    /// Show daemon status
    Status,
    /// Tail the daemon log file
    Logs {
        /// Number of lines to show
        #[arg(short, default_value = "50")]
        lines: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Determine working directory
    let working_dir = match cli.directory {
        Some(dir) => std::fs::canonicalize(dir)?,
        None => std::env::current_dir()?,
    };

    // Load settings
    let mut settings = Settings::load(&working_dir)?;
    if let Some(model) = cli.model {
        settings.default_model = model;
    }
    if let Some(url) = cli.url {
        settings.ollama_url = url;
    }

    match cli.command {
        Some(Command::Daemon { action }) => handle_daemon(action, settings).await,
        Some(Command::Send { message }) => handle_send(message, settings, working_dir).await,
        None => handle_tui(settings, working_dir, cli.standalone, cli.connect).await,
    }
}

// ---------------------------------------------------------------------------
// Daemon subcommands
// ---------------------------------------------------------------------------

async fn handle_daemon(action: DaemonAction, settings: Settings) -> Result<()> {
    match action {
        DaemonAction::Start { foreground } => {
            if daemon::lifecycle::is_running() {
                eprintln!("Daemon is already running (PID: {:?})", daemon::lifecycle::read_pid());
                return Ok(());
            }

            if !foreground {
                println!("Starting Enki daemon...");
            }

            // Daemonize (forks if not foreground)
            daemon::lifecycle::daemonize(foreground)?;

            // Initialize logging
            init_daemon_logging();

            tracing::info!("Enki daemon starting");

            let socket_path = daemon::lifecycle::socket_path();
            let server = daemon::server::DaemonServer::new(settings, 10);

            // Run server with signal handling
            tokio::select! {
                result = server.run(&socket_path) => {
                    if let Err(e) = result {
                        tracing::error!("Server error: {}", e);
                    }
                }
                _ = daemon::lifecycle::shutdown_signal() => {
                    tracing::info!("Shutting down daemon");
                }
            }

            daemon::lifecycle::cleanup_files();
            tracing::info!("Enki daemon stopped");
            Ok(())
        }

        DaemonAction::Stop => {
            if !daemon::lifecycle::is_running() {
                eprintln!("Daemon is not running.");
                return Ok(());
            }

            println!("Stopping Enki daemon...");
            daemon::lifecycle::stop_daemon()?;
            println!("Daemon stopped.");
            Ok(())
        }

        DaemonAction::Status => {
            if daemon::lifecycle::is_running() {
                let pid = daemon::lifecycle::read_pid().unwrap_or(0);
                println!("Enki daemon is running (PID: {})", pid);

                // Try to get session info
                if let Ok(mut client) = daemon::client::DaemonClient::connect().await {
                    let _ = client.send(&daemon::protocol::ClientMessage::ListSessions).await;
                    if let Some(daemon::protocol::ServerMessage::SessionList { sessions }) =
                        client.recv().await
                    {
                        if sessions.is_empty() {
                            println!("No active sessions.");
                        } else {
                            println!("Active sessions:");
                            for s in &sessions {
                                println!(
                                    "  {} — {} ({})",
                                    &s.session_id[..8],
                                    s.working_dir.display(),
                                    s.model_name,
                                );
                            }
                        }
                    }
                }
            } else {
                println!("Enki daemon is not running.");
            }
            Ok(())
        }

        DaemonAction::Logs { lines } => {
            let log_file = daemon::lifecycle::log_path();
            if !log_file.exists() {
                eprintln!("No log file found at {}", log_file.display());
                return Ok(());
            }

            let content = std::fs::read_to_string(&log_file)?;
            let all_lines: Vec<&str> = content.lines().collect();
            let start = all_lines.len().saturating_sub(lines);
            for line in &all_lines[start..] {
                println!("{}", line);
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// One-shot send
// ---------------------------------------------------------------------------

async fn handle_send(message: String, _settings: Settings, working_dir: PathBuf) -> Result<()> {
    let mut client = daemon::client::DaemonClient::connect()
        .await
        .map_err(|_| anyhow::anyhow!("Cannot connect to daemon. Is it running? Try: enki daemon start"))?;

    // Create session
    client
        .send(&daemon::protocol::ClientMessage::CreateSession {
            working_dir,
        })
        .await?;

    let session_id = match client.recv().await {
        Some(daemon::protocol::ServerMessage::SessionCreated { session_id }) => session_id,
        Some(daemon::protocol::ServerMessage::Error { message }) => {
            anyhow::bail!("Failed to create session: {}", message);
        }
        _ => anyhow::bail!("Unexpected response from daemon"),
    };

    // Attach to events
    client
        .send(&daemon::protocol::ClientMessage::AttachSession {
            session_id: session_id.clone(),
        })
        .await?;
    let _ = client.recv().await; // Ok

    // Send message
    client
        .send(&daemon::protocol::ClientMessage::SendMessage {
            session_id: session_id.clone(),
            text: message,
        })
        .await?;
    let _ = client.recv().await; // Ok

    // Read events until TurnComplete
    loop {
        match client.recv().await {
            Some(daemon::protocol::ServerMessage::Event { event, .. }) => {
                use crate::agent::r#loop::AgentEvent;
                match event {
                    AgentEvent::TextDelta(text) => {
                        print!("{}", text);
                    }
                    AgentEvent::Thinking(text) => {
                        eprintln!("[thinking] {}", text);
                    }
                    AgentEvent::ToolCallStart { name, .. } => {
                        eprintln!("[tool] Calling {}...", name);
                    }
                    AgentEvent::ToolResult {
                        name,
                        result,
                        is_error,
                    } => {
                        if is_error {
                            eprintln!("[tool] {} ERROR: {}", name, result);
                        } else {
                            eprintln!("[tool] {} → {}", name, &result[..result.len().min(200)]);
                        }
                    }
                    AgentEvent::NeedsApproval { tool_name, description, .. } => {
                        eprintln!("[approval] {} — {} (auto-denied in one-shot mode)", tool_name, description);
                        let _ = client
                            .send(&daemon::protocol::ClientMessage::ApproveAction {
                                session_id: session_id.clone(),
                                approved: false,
                            })
                            .await;
                    }
                    AgentEvent::Complete(text) => {
                        if !text.is_empty() {
                            println!("{}", text);
                        }
                    }
                    AgentEvent::Error(err) => {
                        eprintln!("[error] {}", err);
                    }
                    AgentEvent::TurnComplete => break,
                    _ => {}
                }
            }
            None => break, // Connection closed
            _ => {}
        }
    }

    println!();
    Ok(())
}

// ---------------------------------------------------------------------------
// TUI mode
// ---------------------------------------------------------------------------

async fn handle_tui(
    settings: Settings,
    working_dir: PathBuf,
    _standalone: bool,
    force_connect: bool,
) -> Result<()> {
    // For now, always run standalone TUI.
    // Remote TUI mode (connecting to daemon) will be implemented in a follow-up.
    if force_connect {
        anyhow::bail!("Remote TUI mode (--connect) is not yet implemented. Use standalone mode.");
    }

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let result = run_app(&mut terminal, settings, working_dir).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(ref e) = result {
        eprintln!("Error: {:#}", e);
    }

    result
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    settings: Settings,
    working_dir: PathBuf,
) -> Result<()> {
    let mut app = app::App::new(settings, working_dir).await?;
    app.run(terminal).await
}

// ---------------------------------------------------------------------------
// Daemon logging setup
// ---------------------------------------------------------------------------

fn init_daemon_logging() {
    use tracing_subscriber::fmt;
    use tracing_subscriber::EnvFilter;

    let log_path = daemon::lifecycle::log_path();
    let log_dir = log_path.parent().unwrap_or(std::path::Path::new("/tmp"));
    let log_file = log_path.file_name().unwrap_or_default();

    let file_appender = tracing_appender::rolling::daily(log_dir, log_file);

    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(file_appender)
        .with_ansi(false)
        .init();
}
