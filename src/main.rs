mod cli;
mod commands;
mod crypto;
mod info;
mod paths;
mod prompt;
mod secret;
mod store;
mod tui;
mod vault;

use std::process::ExitCode;

use clap::Parser;
use cli::{Cli, Command};

fn main() -> ExitCode {
    harden_process();
    let cli = Cli::parse();
    let vault_path = vault::resolve_path(cli.vault.as_deref()).ok();
    match cli.command {
        Some(Command::Version) => println!("maxonkey {}", env!("CARGO_PKG_VERSION")),
        Some(Command::About) => println!("{}", info::ABOUT),
        Some(Command::Status) => print!("{}", info::status(vault_path.as_deref())),
        Some(Command::Vault(command)) => {
            if let Err(error) = commands::run(command, cli.vault.as_deref()) {
                eprintln!("maxonkey: {error}");
                return ExitCode::from(error.exit_code());
            }
        }
        None => {
            if let Err(error) = tui::run(info::vault_line(vault_path.as_deref())) {
                eprintln!("maxonkey: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}

/// Linux: no core dumps, and other same-user processes cannot ptrace this process or
/// read `/proc/<pid>/mem` (SR-026). Applied before any passphrase is read.
fn harden_process() {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: PR_SET_DUMPABLE takes plain integer arguments and has no memory effects.
        unsafe {
            libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
        }
    }
}
