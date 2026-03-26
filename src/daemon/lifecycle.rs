use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

/// Resolve the runtime directory for socket and PID files.
/// Uses $XDG_RUNTIME_DIR/enki/ if available, otherwise /tmp/enki-$UID/.
pub fn runtime_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(xdg).join("enki")
    } else {
        let uid = nix::unistd::getuid();
        PathBuf::from(format!("/tmp/enki-{}", uid))
    }
}

/// Get the path to the daemon Unix socket.
pub fn socket_path() -> PathBuf {
    runtime_dir().join("enki.sock")
}

/// Get the path to the daemon PID file.
pub fn pid_path() -> PathBuf {
    runtime_dir().join("enki.pid")
}

/// Get the path to the daemon log file.
pub fn log_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("enki")
        .join("daemon.log")
}

/// Write the current process PID to the PID file.
pub fn write_pid_file() -> Result<()> {
    let path = pid_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, std::process::id().to_string())
        .with_context(|| format!("Failed to write PID file: {}", path.display()))
}

/// Read the PID from the PID file, if it exists.
pub fn read_pid() -> Option<u32> {
    let path = pid_path();
    fs::read_to_string(&path)
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Check if a daemon process is currently running.
pub fn is_running() -> bool {
    if let Some(pid) = read_pid() {
        // Check if the process exists by sending signal 0
        let pid = nix::unistd::Pid::from_raw(pid as i32);
        nix::sys::signal::kill(pid, None).is_ok()
    } else {
        false
    }
}

/// Stop a running daemon by sending SIGTERM.
pub fn stop_daemon() -> Result<()> {
    let pid = read_pid().context("No PID file found — daemon may not be running")?;
    let pid = nix::unistd::Pid::from_raw(pid as i32);

    nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGTERM)
        .context("Failed to send SIGTERM to daemon process")?;

    // Wait briefly for cleanup
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if nix::sys::signal::kill(pid, None).is_err() {
            // Process is gone
            cleanup_files();
            return Ok(());
        }
    }

    // Force kill after 2 seconds
    let _ = nix::sys::signal::kill(pid, nix::sys::signal::Signal::SIGKILL);
    cleanup_files();
    Ok(())
}

/// Remove socket and PID files.
pub fn cleanup_files() {
    let _ = fs::remove_file(socket_path());
    let _ = fs::remove_file(pid_path());
}

/// Daemonize the current process (fork + setsid + detach).
/// In `foreground` mode, skip daemonization (useful for systemd / debugging).
pub fn daemonize(foreground: bool) -> Result<()> {
    if foreground {
        // Just write PID file, don't fork
        write_pid_file()?;
        return Ok(());
    }

    let log_file_path = log_path();
    if let Some(parent) = log_file_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let stdout = fs::File::create(&log_file_path)
        .with_context(|| format!("Failed to create log file: {}", log_file_path.display()))?;
    let stderr = stdout
        .try_clone()
        .context("Failed to clone log file handle")?;

    let pid_file_path = pid_path();
    if let Some(parent) = pid_file_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let daemonize = daemonize::Daemonize::new()
        .pid_file(&pid_file_path)
        .chown_pid_file(true)
        .working_directory("/")
        .stdout(stdout)
        .stderr(stderr)
        .umask(0o077);

    daemonize
        .start()
        .map_err(|e| anyhow::anyhow!("Failed to daemonize: {}", e))?;

    Ok(())
}

/// Set up signal handlers for graceful shutdown.
/// Returns a future that resolves when a shutdown signal is received.
pub async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};

    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
    let mut sigint = signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");

    tokio::select! {
        _ = sigterm.recv() => {
            tracing::info!("Received SIGTERM");
        }
        _ = sigint.recv() => {
            tracing::info!("Received SIGINT");
        }
    }

    cleanup_files();
}
