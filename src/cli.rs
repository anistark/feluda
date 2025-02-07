use clap::{Parser, ArgGroup};
use std::io::{self, Write};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
#[command(group(ArgGroup::new("output").args(["json"])))]
pub struct Cli {
    /// Path to the local project directory
    #[arg(short, long, default_value = "./")]
    pub path: String,

    /// Output in JSON format
    #[arg(long)]
    pub json: bool,

    /// Enable verbose output
    #[arg(long)]
    pub verbose: bool,

    /// Show only restrictive dependencies in strict mode
    #[arg(long)]
    pub strict: bool,

    /// Enable TUI table
    #[arg(long, short)]
    pub gui: bool,

    /// Enable debug mode
    #[arg(long)]
    pub debug: bool,
}

pub fn clear_last_line() {
    print!("\x1b[1A\x1b[2K");
    io::stdout().flush().unwrap();
}
