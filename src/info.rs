use std::path::Path;

use crate::vault::{self, VaultStatus};

pub const ABOUT: &str = concat!(
    "MaxonKey ",
    env!("CARGO_PKG_VERSION"),
    "\nMaxon Torres Software\n\n",
    "Personal, local-first secrets manager.\n",
    "Windows first; Linux and macOS later.\n\n",
    "Encrypted vault prototype: stores secrets by\n",
    "project and environment from the CLI.\n",
    "Backup, restore, and key rotation are not available yet."
);

pub const IMPLEMENTED: [&str; 4] = [
    "Vault init / passphrase / recovery",
    "Encryption / SQLite storage",
    "Secret CRUD (CLI)",
    "Projects / environments",
];

pub const UNAVAILABLE: [&str; 4] = [
    "Key rotation / backup / restore",
    "MCP server",
    "MCP owner approval",
    "Secure sharing",
];

/// One truthful line about the vault. Never claims "unlocked": no process keeps it open.
pub fn vault_line(path: Option<&Path>) -> String {
    let Some(path) = path else {
        return "Vault: UNKNOWN (no data directory; use --vault)".into();
    };
    match vault::status(path) {
        Ok(VaultStatus::NotInitialized) => "Vault: NOT INITIALIZED".into(),
        Ok(VaultStatus::Initialized { .. }) => "Vault: INITIALIZED (LOCKED)".into(),
        Err(_) => "Vault: DAMAGED OR UNREADABLE (run maxonkey vault status)".into(),
    }
}

pub fn status(vault_path: Option<&Path>) -> String {
    let mut output = String::from("MaxonKey prototype\nCLI / TUI: IMPLEMENTED\n");
    for label in IMPLEMENTED {
        output.push_str(&format!("{label}: IMPLEMENTED\n"));
    }
    for label in UNAVAILABLE {
        output.push_str(&format!("{label}: NOT IMPLEMENTED\n"));
    }
    output.push_str(&vault_line(vault_path));
    output.push('\n');
    if let Some(path) = vault_path {
        output.push_str(&format!("Vault path: {}\n", path.display()));
    }
    output.push_str("Secrets are managed with: maxonkey secret --help\n");
    output
}
