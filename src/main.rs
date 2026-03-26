#![allow(dead_code, unused_imports)]

mod agent;
mod app;
mod config;
mod llm;
mod tools;
mod tui;

use std::io;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
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
    #[arg(short = 'C', long)]
    directory: Option<PathBuf>,

    /// Ollama model to use
    #[arg(short, long)]
    model: Option<String>,

    /// Ollama server URL
    #[arg(long)]
    url: Option<String>,
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
