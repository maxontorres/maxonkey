//! `maxonkey vault …` and `maxonkey secret …` command handlers.

use std::{
    io::{self, IsTerminal, Write},
    path::Path,
};

use zeroize::Zeroizing;

use crate::{
    cli::{SecretCommand, VaultCommand},
    crypto::KdfParams,
    paths, prompt,
    vault::{self, Address, Credential, MAX_VALUE_LEN, Unlocked, VaultError, VaultStatus},
};

pub fn run_secret(command: SecretCommand, explicit: Option<&Path>) -> Result<(), VaultError> {
    let path = vault::resolve_path(explicit)?;
    match command {
        SecretCommand::Set {
            project,
            environment,
            name,
            stdin,
        } => set(&path, &address(&project, &environment, &name)?, stdin),
        SecretCommand::Get {
            project,
            environment,
            name,
        } => get(&path, &address(&project, &environment, &name)?),
        SecretCommand::List {
            project,
            environment,
        } => list(&path, project.as_deref(), environment.as_deref()),
        SecretCommand::Delete {
            project,
            environment,
            name,
            yes,
        } => delete(&path, &address(&project, &environment, &name)?, yes),
    }
}

/// Validates the address before any prompt, so a typo never costs a passphrase entry.
fn address<'a>(
    project: &'a str,
    environment: &'a str,
    name: &'a str,
) -> Result<Address<'a>, VaultError> {
    vault::check_name(project, "project")?;
    vault::check_name(environment, "environment")?;
    vault::check_name(name, "secret name")?;
    Ok(Address {
        project,
        environment,
        name,
    })
}

/// Asks for the passphrase and unlocks; the passphrase is wiped right after.
fn unlock_with_passphrase(path: &Path) -> Result<Unlocked, VaultError> {
    if path.symlink_metadata().is_err() {
        return Err(VaultError::NotInitialized(path.to_path_buf()));
    }
    let passphrase = prompt::passphrase("Master passphrase: ")?;
    let unlocked = vault::unlock(path, Credential::Passphrase(&passphrase))?;
    drop(passphrase);
    warn_permissions(path);
    Ok(unlocked)
}

fn set(path: &Path, address: &Address<'_>, from_stdin: bool) -> Result<(), VaultError> {
    // Read piped input first so the vault stays locked while waiting for it.
    let piped = if from_stdin {
        let bytes = prompt::value_from_stdin(MAX_VALUE_LEN)?;
        if bytes.is_empty() {
            return Err(VaultError::Usage(
                "the secret value must be between 1 byte and 64 KiB".into(),
            ));
        }
        Some(bytes)
    } else {
        None
    };
    let mut unlocked = unlock_with_passphrase(path)?;
    let typed;
    let value: &[u8] = match &piped {
        Some(bytes) => bytes,
        None => {
            typed = prompt::hidden("Secret value (hidden): ")?;
            typed.as_bytes()
        }
    };
    let created = unlocked.set(address, value)?;
    println!(
        "Secret {}. Vault generation: {}",
        if created { "created" } else { "updated" },
        unlocked.generation()
    );
    Ok(())
}

fn get(path: &Path, address: &Address<'_>) -> Result<(), VaultError> {
    let unlocked = unlock_with_passphrase(path)?;
    let value = unlocked.get(address)?;
    drop(unlocked);
    let stdout = io::stdout();
    let newline = stdout.is_terminal();
    let mut out = stdout.lock();
    out.write_all(value.expose())?;
    if newline {
        out.write_all(b"\n")?;
    }
    out.flush()?;
    Ok(())
}

fn list(path: &Path, project: Option<&str>, environment: Option<&str>) -> Result<(), VaultError> {
    if let Some(project) = project {
        vault::check_name(project, "project")?;
    }
    if let Some(environment) = environment {
        vault::check_name(environment, "environment")?;
    }
    let unlocked = unlock_with_passphrase(path)?;
    let found = unlocked.list(project, environment);
    if found.is_empty() {
        eprintln!("No secrets found.");
        return Ok(());
    }
    let mut out = io::stdout().lock();
    for address in found {
        writeln!(
            out,
            "{}\t{}\t{}",
            address.project, address.environment, address.name
        )?;
    }
    out.flush()?;
    Ok(())
}

fn delete(path: &Path, address: &Address<'_>, skip_confirmation: bool) -> Result<(), VaultError> {
    let mut unlocked = unlock_with_passphrase(path)?;
    if !unlocked.contains(address)? {
        return Err(VaultError::NotFound);
    }
    if !skip_confirmation && !prompt::confirm("Delete this secret permanently? [y/N] ")? {
        return Err(VaultError::Input(
            "deletion cancelled; nothing was changed".into(),
        ));
    }
    unlocked.delete(address)?;
    println!(
        "Secret deleted. Vault generation: {}",
        unlocked.generation()
    );
    Ok(())
}

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
