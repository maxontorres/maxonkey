//! `maxonkey vault …` command handlers.

use std::path::Path;

use zeroize::Zeroizing;

use crate::{
    cli::VaultCommand,
    crypto::KdfParams,
    paths, prompt,
    vault::{self, Credential, VaultError, VaultStatus},
};

pub fn run(command: VaultCommand, explicit: Option<&Path>) -> Result<(), VaultError> {
    let path = vault::resolve_path(explicit)?;
    match command {
        VaultCommand::Init { no_recovery_key } => init(&path, !no_recovery_key),
        VaultCommand::Status => status(&path),
        VaultCommand::ChangePassphrase => change_passphrase(&path),
        VaultCommand::Recover => recover(&path),
    }
}

fn init(path: &Path, with_recovery: bool) -> Result<(), VaultError> {
    if path.symlink_metadata().is_ok() {
        return Err(VaultError::AlreadyExists(path.to_path_buf()));
    }
    prompt::to_terminal(&format!(
        "Creating a vault at {}\nChoose a master passphrase of at least 12 characters. \
         Four or more random words work well.\nThere is no way to reset it without the recovery key.\n",
        path.display()
    ))?;
    let passphrase = prompt::new_passphrase("New master passphrase: ")?;
    let recovery = if with_recovery {
        let key = vault::new_recovery_key()?;
        let mut text = Zeroizing::new(String::from(
            "\nRecovery key (shown once). Write it down and keep it offline, away from this computer.\n\
             Anyone with this key can open your vault.\n\n    ",
        ));
        text.push_str(&vault::format_recovery_key(&key));
        text.push_str("\n\n");
        prompt::to_terminal(&text)?;
        prompt::hidden("Press Enter after you have stored the recovery key: ")?;
        prompt::to_terminal("Clear your terminal scrollback so the key is not left on screen.\n")?;
        Some(key)
    } else {
        None
    };
    vault::init(path, &passphrase, recovery.as_ref(), KdfParams::DEFAULT)?;
    println!("Vault created: {}", path.display());
    if !with_recovery {
        eprintln!(
            "warning: no recovery key; if you forget the passphrase the vault cannot be opened"
        );
    }
    Ok(())
}

fn status(path: &Path) -> Result<(), VaultError> {
    match vault::status(path)? {
        VaultStatus::NotInitialized => {
            println!("Vault: NOT INITIALIZED");
            println!("Path: {}", path.display());
            println!("Create one with: maxonkey vault init");
        }
        VaultStatus::Initialized {
            format_version,
            params,
            recovery_key,
            warnings,
        } => {
            println!("Vault: INITIALIZED (LOCKED)");
            println!("Path: {}", path.display());
            println!("Format version: {format_version}");
            match params {
                Some(p) => println!(
                    "Key derivation: Argon2id m={} KiB t={} p={}",
                    p.m_kib, p.t, p.p
                ),
                None => println!("Key derivation: MISSING"),
            }
            println!(
                "Recovery key: {}",
                if recovery_key {
                    "CONFIGURED"
                } else {
                    "NOT CONFIGURED"
                }
            );
            println!("File structure check: PASSED (contents are verified at unlock)");
            if warnings.is_empty() {
                println!("Permissions: OWNER ONLY");
            } else {
                for warning in warnings {
                    println!("Permissions: WARNING: {warning}");
                }
            }
        }
    }
    Ok(())
}

fn warn_permissions(path: &Path) {
    for warning in paths::permission_warnings(path) {
        eprintln!("warning: {warning}");
    }
}

fn change_passphrase(path: &Path) -> Result<(), VaultError> {
    if path.symlink_metadata().is_err() {
        return Err(VaultError::NotInitialized(path.to_path_buf()));
    }
    let current = prompt::passphrase("Current master passphrase: ")?;
    let mut unlocked = vault::unlock(path, Credential::Passphrase(&current))?;
    drop(current);
    warn_permissions(path);
    let new = prompt::new_passphrase("New master passphrase: ")?;
    unlocked.change_passphrase(&new, KdfParams::DEFAULT)?;
    println!(
        "Passphrase changed. Vault generation: {}",
        unlocked.generation()
    );
    eprintln!("note: backups or copies made earlier still open with the old passphrase");
    Ok(())
}

fn recover(path: &Path) -> Result<(), VaultError> {
    match vault::status(path)? {
        VaultStatus::NotInitialized => return Err(VaultError::NotInitialized(path.to_path_buf())),
        VaultStatus::Initialized {
            recovery_key: false,
            ..
        } => return Err(VaultError::NoRecoveryKey),
        VaultStatus::Initialized { .. } => {}
    }
    let input = prompt::hidden("Recovery key: ")?;
    let key = vault::parse_recovery_key(&input)?;
    drop(input);
    let mut unlocked = vault::unlock(path, Credential::RecoveryKey(&key))?;
    drop(key);
    warn_permissions(path);
    let new = prompt::new_passphrase("New master passphrase: ")?;
    unlocked.change_passphrase(&new, KdfParams::DEFAULT)?;
    println!(
        "Passphrase reset. The recovery key remains valid. Vault generation: {}",
        unlocked.generation()
    );
    Ok(())
}
