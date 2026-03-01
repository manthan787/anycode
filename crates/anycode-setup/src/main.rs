mod app;
mod config_gen;
mod data;
mod runner;
mod steps;
mod widgets;

use std::io;
use std::process::Command;

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

fn main() -> anyhow::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the app
    let mut app = app::App::new();
    let should_run = app.run(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    match should_run {
        Ok(true) => {
            println!("Starting anycode...");
            let status = Command::new("./target/release/anycode")
                .arg("--config")
                .arg("config.toml")
                .status();
            match status {
                Ok(s) => std::process::exit(s.code().unwrap_or(1)),
                Err(e) => {
                    eprintln!("Failed to start anycode: {e}");
                    std::process::exit(1);
                }
            }
        }
        Ok(false) => {
            println!("Setup complete. Run: ./target/release/anycode --config config.toml");
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }

    Ok(())
}
