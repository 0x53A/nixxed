mod app;
mod config_parser;
mod search;

use anyhow::{Context, Result};
use app::App;
use config_parser::NixConfig;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::PathBuf;

fn main() -> Result<()> {
    // Find the NixOS configuration file
    let config_path = find_config_path()?;

    println!("Loading NixOS configuration from: {}", config_path.display());

    // Load the configuration
    let config = NixConfig::load(&config_path)?;

    // Setup terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)
        .context("Failed to setup terminal")?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("Failed to create terminal")?;

    // Create and run the app
    let mut app = App::new(config);
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode().context("Failed to disable raw mode")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .context("Failed to restore terminal")?;
    terminal.show_cursor().context("Failed to show cursor")?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        // Poll for background search results
        app.poll_search();
        
        terminal.draw(|f| app.draw(f))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            let event = event::read()?;
            app.handle_event(event)?;

            if app.should_quit {
                break;
            }
        }
    }

    Ok(())
}

fn find_config_path() -> Result<PathBuf> {
    // Check command line argument first
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let path = PathBuf::from(&args[1]);
        if path.exists() {
            return Ok(path);
        } else {
            anyhow::bail!("Configuration file not found: {}", path.display());
        }
    }

    // Try common NixOS configuration paths
    let common_paths = [
        PathBuf::from("/etc/nixos/configuration.nix"),
        PathBuf::from("/etc/nixos/hardware-configuration.nix"),
    ];

    // Also check for home-manager config if using it
    if let Some(home) = dirs::home_dir() {
        let home_manager_paths = [
            home.join(".config/nixpkgs/home.nix"),
            home.join(".config/home-manager/home.nix"),
        ];

        for path in home_manager_paths {
            if path.exists() {
                return Ok(path);
            }
        }
    }

    for path in common_paths {
        if path.exists() {
            return Ok(path);
        }
    }

    anyhow::bail!(
        "Could not find NixOS configuration file. \
         Please specify the path as a command line argument:\n\
         nixxed /path/to/configuration.nix"
    )
}

