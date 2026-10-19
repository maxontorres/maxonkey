mod cli;
mod info;
mod tui;

use std::process::ExitCode;

use clap::Parser;
use cli::{Cli, Command};

fn main() -> ExitCode {
    match Cli::parse().command {
        Some(Command::Version) => println!("maxonkey {}", env!("CARGO_PKG_VERSION")),
        Some(Command::About) => println!("{}", info::ABOUT),
        Some(Command::Status) => print!("{}", info::status()),
        None => {
            if let Err(error) = tui::run() {
                eprintln!("maxonkey: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
