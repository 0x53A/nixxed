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
        // Check if we need to run nixos-rebuild
        if app.rebuild_prompt.pending_rebuild {
            app.rebuild_prompt.pending_rebuild = false;
            run_nixos_rebuild(terminal, app)?;
            continue;
        }

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

    // Drain any remaining events from the input buffer to prevent
    // escape sequence characters from leaking into the shell
    while event::poll(std::time::Duration::from_millis(10))? {
        let _ = event::read();
    }

    Ok(())
}

/// Run nixos-rebuild switch with live output by temporarily leaving the TUI
fn run_nixos_rebuild(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    // Leave the alternate screen to show live output
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    println!("\n\x1b[1;36m═══════════════════════════════════════════════════════════════\x1b[0m");
    println!("\x1b[1;36m  Running: sudo nixos-rebuild switch\x1b[0m");
    println!("\x1b[1;36m═══════════════════════════════════════════════════════════════\x1b[0m\n");

    // Run the command with inherited stdio for live output
    let status = std::process::Command::new("sudo")
        .args(["nixos-rebuild", "switch"])
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status();

    let (success, message) = match status {
        Ok(exit_status) => {
            if exit_status.success() {
                println!("\n\x1b[1;32m✓ Rebuild completed successfully!\x1b[0m");
                (true, "Rebuild completed successfully!".to_string())
            } else {
                let code = exit_status.code().unwrap_or(-1);
                println!("\n\x1b[1;31m✗ Rebuild failed with exit code {}\x1b[0m", code);
                (false, format!("Rebuild failed with exit code {}", code))
            }
        }
        Err(e) => {
            println!("\n\x1b[1;31m✗ Failed to run nixos-rebuild: {}\x1b[0m", e);
            (false, format!("Failed to run nixos-rebuild: {}", e))
        }
    };

    println!("\n\x1b[90mPress Enter to return to nixxed...\x1b[0m");
    
    // Wait for user to press Enter
    let mut input = String::new();
    let _ = std::io::stdin().read_line(&mut input);

    // Re-enter the alternate screen
    enable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    terminal.hide_cursor()?;
    terminal.clear()?;

    // Close the rebuild prompt and update status
    app.rebuild_prompt.show = false;
    app.status_message = Some(if success {
        "System rebuilt successfully!".to_string()
    } else {
        message
    });

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

