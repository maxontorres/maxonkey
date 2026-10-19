use std::process::{Command, Output, Stdio};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_maxonkey"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .expect("run maxonkey")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("UTF-8 output")
}

#[test]
fn help_forms_list_commands_and_default_behavior() {
    for args in [vec!["help"], vec!["--help"], vec!["-h"]] {
        let output = run(&args);
        assert!(output.status.success());
        let text = stdout(&output);
        for expected in ["Usage:", "about", "status", "version", "TUI"] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn version_forms_agree() {
    for args in [vec!["version"], vec!["--version"], vec!["-V"]] {
        let output = run(&args);
        assert!(output.status.success());
        assert_eq!(stdout(&output), "maxonkey 0.1.0\n");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn about_identifies_product_and_vendor() {
    let output = run(&["about"]);
    assert!(output.status.success());
    let text = stdout(&output);
    for expected in ["MaxonKey 0.1.0", "Maxon Torres Software", "prototype"] {
        assert!(text.contains(expected));
    }
}

fn temp_vault() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("vault.sqlite3").display().to_string();
    (dir, path)
}

#[test]
fn status_reports_capabilities_and_vault_state() {
    let (_dir, vault) = temp_vault();
    let output = run(&["status", "--vault", &vault]);
    assert!(output.status.success());
    let text = stdout(&output);
    for label in [
        "Vault init / passphrase / recovery",
        "Encryption / SQLite storage",
    ] {
        assert!(
            text.lines()
                .any(|line| line == format!("{label}: IMPLEMENTED"))
        );
    }
    for label in [
        "Secret CRUD",
        "Backup / restore",
        "MCP server",
        "MCP owner approval",
    ] {
        assert!(
            text.lines()
                .any(|line| line == format!("{label}: NOT IMPLEMENTED"))
        );
    }
    assert!(text.contains("Vault: NOT INITIALIZED"));
    assert!(!text.contains("UNLOCKED"));
    assert!(text.contains("Secrets cannot be stored yet"));
}

#[test]
fn vault_status_without_vault_needs_no_passphrase() {
    let (_dir, vault) = temp_vault();
    let output = run(&["vault", "status", "--vault", &vault]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("Vault: NOT INITIALIZED"));
    assert!(text.contains("maxonkey vault init"));
    assert!(
        !std::path::Path::new(&vault).exists(),
        "status must not create files"
    );
}

#[test]
fn vault_commands_fail_before_prompting_when_preconditions_fail() {
    let (dir, vault) = temp_vault();
    // Missing vault: exit 5 without asking for a passphrase.
    for command in ["change-passphrase", "recover"] {
        let output = run(&["vault", command, "--vault", &vault]);
        assert_eq!(output.status.code(), Some(5), "{command}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("maxonkey vault init"));
    }
    // Existing file: init refuses and leaves it unchanged (ST-10).
    std::fs::write(&vault, b"existing content").unwrap();
    let output = run(&["vault", "init", "--vault", &vault]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("already exists"));
    assert_eq!(std::fs::read(&vault).unwrap(), b"existing content");
    // Not a vault: status reports an integrity failure (exit 4).
    let output = run(&["vault", "status", "--vault", &vault]);
    assert_eq!(output.status.code(), Some(4));
    let output = run(&["status", "--vault", &vault]);
    assert!(stdout(&output).contains("Vault: DAMAGED OR UNREADABLE"));
    drop(dir);
}

#[test]
fn credentials_cannot_be_passed_as_arguments() {
    let (_dir, vault) = temp_vault();
    for option in ["--passphrase", "--password", "--recovery-key", "--value"] {
        let output = run(&["vault", "init", "--vault", &vault, option, "MK-CANARY-arg"]);
        assert_eq!(output.status.code(), Some(2), "{option}");
        assert!(!std::path::Path::new(&vault).exists());
    }
}

#[test]
fn invalid_input_fails_without_starting_tui() {
    for args in [
        vec!["unlock"],
        vec!["--bogus"],
        vec!["status", "unexpected"],
    ] {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(2));
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("error:"));
        assert!(error.contains("--help"));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn no_command_without_terminal_fails_with_guidance() {
    let output = run(&[]);
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("interactive terminal"));
    assert!(error.contains("maxonkey --help"));
    assert!(!output.stdout.contains(&0x1b));
}
