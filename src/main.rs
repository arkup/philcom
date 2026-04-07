use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

mod app;
mod config;
mod editor;
mod highlight;
mod panel;
mod theme;
mod ui;

use app::App;

struct Args {
    left: String,
    right: String,
}

fn parse_args() -> Args {
    let raw: Vec<String> = std::env::args().skip(1).collect();

    if raw.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("Usage: philcom [left-dir] [right-dir]");
        eprintln!("       philcom --left <dir> --right <dir>");
        eprintln!("       philcom -l <dir> -r <dir>");
        std::process::exit(0);
    }

    let get_flag = |names: &[&str]| -> Option<String> {
        raw.windows(2).find(|w| names.contains(&w[0].as_str())).map(|w| w[1].clone())
    };

    // Named flags take priority
    if let (Some(l), Some(r)) = (get_flag(&["--left", "-l"]), get_flag(&["--right", "-r"])) {
        return Args { left: l, right: r };
    }
    if let Some(l) = get_flag(&["--left", "-l"]) {
        return Args { left: l, right: ".".into() };
    }
    if let Some(r) = get_flag(&["--right", "-r"]) {
        return Args { left: ".".into(), right: r };
    }

    // Positional fallback (skip anything starting with '-')
    let pos: Vec<&str> = raw.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
    Args {
        left:  pos.first().copied().unwrap_or(".").to_string(),
        right: pos.get(1).copied().unwrap_or(".").to_string(),
    }
}

fn main() -> Result<()> {
    let args = parse_args();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(&args.left, &args.right)?;
    let result = app.run(&mut terminal);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }

    Ok(())
}
