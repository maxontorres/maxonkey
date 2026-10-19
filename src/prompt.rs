//! Hidden input and private output on the controlling terminal (SR-020, SR-021).
//!
//! Passphrases and recovery keys are read from `/dev/tty` (Unix) or `CONIN$` (Windows),
//! never from arguments, environment variables, or standard input.

use std::io::{self, Write};

use zeroize::Zeroizing;

use crate::{
    secret::Passphrase,
    vault::{self, VaultError},
};

fn terminal_error(error: io::Error) -> VaultError {
    VaultError::Terminal(error)
}

/// Reads one line from the terminal with echo disabled.
pub fn hidden(prompt: &str) -> Result<Zeroizing<String>, VaultError> {
    rpassword::prompt_password(prompt)
        .map(Zeroizing::new)
        .map_err(terminal_error)
}

pub fn passphrase(prompt: &str) -> Result<Passphrase, VaultError> {
    Ok(Passphrase::from_input(hidden(prompt)?))
}

/// Asks twice and applies the minimum-length rule.
pub fn new_passphrase(first: &str) -> Result<Passphrase, VaultError> {
    let passphrase = passphrase(first)?;
    vault::check_new_passphrase(&passphrase)?;
    let again = self::passphrase("Repeat the passphrase: ")?;
    if !passphrase.same_as(&again) {
        return Err(VaultError::Input("the passphrases did not match".into()));
    }
    Ok(passphrase)
}

/// Writes directly to the terminal, bypassing redirected stdout, so the text is never
/// captured in a file or pipe.
pub fn to_terminal(text: &str) -> Result<(), VaultError> {
    #[cfg(unix)]
    const DEVICE: &str = "/dev/tty";
    #[cfg(windows)]
    const DEVICE: &str = "CONOUT$";
    let mut terminal = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(DEVICE)
        .map_err(terminal_error)?;
    terminal
        .write_all(text.as_bytes())
        .map_err(terminal_error)?;
    terminal.flush().map_err(terminal_error)
}
