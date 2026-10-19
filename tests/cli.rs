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

#[test]
fn status_reports_every_unimplemented_capability() {
    let output = run(&["status"]);
    assert!(output.status.success());
    let text = stdout(&output);
    for label in [
        "Encrypted vault",
        "Encryption",
        "SQLite storage",
        "Secret CRUD",
        "MCP server",
    ] {
        assert!(
            text.lines()
                .any(|line| line.contains(label) && line.contains("NOT IMPLEMENTED"))
        );
    }
    assert!(text.contains("No secrets are read or stored"));
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
