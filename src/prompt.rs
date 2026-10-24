//! Hidden input and private output on the controlling terminal (SR-020, SR-021).
//!
//! Passphrases and recovery keys are read from `/dev/tty` (Unix) or `CONIN$` (Windows),
//! never from arguments, environment variables, or standard input.

use std::io::{self, BufRead, Read, Write};

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

/// Asks a yes/no question on the terminal (visible input). Only "y" or "yes" confirms.
pub fn confirm(question: &str) -> Result<bool, VaultError> {
    #[cfg(unix)]
    const DEVICE: &str = "/dev/tty";
    #[cfg(windows)]
    const DEVICE: &str = "CONIN$";
    to_terminal(question)?;
    let input = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(DEVICE)
        .map_err(terminal_error)?;
    let mut line = String::new();
    io::BufReader::new(input)
        .read_line(&mut line)
        .map_err(terminal_error)?;
    let answer = line.trim();
    Ok(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
}

/// Reads a secret value from standard input exactly as given (SR-022), up to `limit` bytes.
/// The buffer is allocated once, so no unwiped copies are left behind.
pub fn value_from_stdin(limit: usize) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let mut buffer = Zeroizing::new(vec![0u8; limit + 1]);
    let mut length = 0;
    let mut stdin = io::stdin().lock();
    while length <= limit {
        match stdin.read(&mut buffer[length..]) {
            Ok(0) => break,
            Ok(read) => length += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(VaultError::Io(error)),
        }
    }
    if length > limit {
        return Err(VaultError::Usage(
            "the secret value must be between 1 byte and 64 KiB".into(),
        ));
    }
    buffer.truncate(length);
    Ok(buffer)
}
