use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "maxonkey",
    version,
    about = "MaxonKey — local-first secrets manager prototype"
)]
#[command(
    after_help = "Run without a command to launch the TUI. Passphrases are read only from the terminal.\nSecret values are read from a hidden prompt or, with --stdin, from standard input."
)]
pub struct Cli {
    /// Use this vault file instead of the standard location
    #[arg(long, global = true, value_name = "PATH")]
    pub vault: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub enum Command {
    /// Print the application version
    Version,
    /// Show product and vendor information
    About,
    /// Report implemented and unavailable capabilities
    Status,
    /// Create and manage the encrypted vault
    #[command(subcommand)]
    Vault(VaultCommand),
    /// Store, read, list, and delete secrets (asks for the passphrase every time)
    #[command(subcommand)]
    Secret(SecretCommand),
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub enum SecretCommand {
    /// Create or update a secret; the value is read from a hidden prompt
    Set {
        project: String,
        environment: String,
        name: String,
        /// Read the value from standard input instead (all bytes, unchanged)
        #[arg(long)]
        stdin: bool,
    },
    /// Print a secret's value to standard output
    Get {
        project: String,
        environment: String,
        name: String,
    },
    /// List secret names (never values), optionally for one project and environment
    List {
        project: Option<String>,
        environment: Option<String>,
    },
    /// Delete a secret after confirmation on the terminal
    Delete {
        project: String,
        environment: String,
        name: String,
        /// Do not ask for confirmation
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Debug, PartialEq, Eq, Subcommand)]
pub enum VaultCommand {
    /// Create a new encrypted vault
    Init {
        /// Do not create a recovery key (a forgotten passphrase then means permanent loss)
        #[arg(long)]
        no_recovery_key: bool,
    },
    /// Show whether a vault exists, without asking for the passphrase
    Status,
    /// Set a new master passphrase (requires the current one)
    ChangePassphrase,
    /// Set a new master passphrase using the recovery key
    Recover,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser, error::ErrorKind};

    #[test]
    fn cli_is_valid_and_defaults_to_tui() {
        Cli::command().debug_assert();
        assert_eq!(Cli::try_parse_from(["maxonkey"]).unwrap().command, None);
    }

    #[test]
    fn parses_informational_commands() {
        for (input, expected) in [
            ("version", Command::Version),
            ("about", Command::About),
            ("status", Command::Status),
        ] {
            assert_eq!(
                Cli::try_parse_from(["maxonkey", input]).unwrap().command,
                Some(expected)
            );
        }
    }

    #[test]
    fn parses_vault_commands_and_global_path() {
        for (args, expected) in [
            (
                vec!["vault", "init"],
                VaultCommand::Init {
                    no_recovery_key: false,
                },
            ),
            (
                vec!["vault", "init", "--no-recovery-key"],
                VaultCommand::Init {
                    no_recovery_key: true,
                },
            ),
            (vec!["vault", "status"], VaultCommand::Status),
            (
                vec!["vault", "change-passphrase"],
                VaultCommand::ChangePassphrase,
            ),
            (vec!["vault", "recover"], VaultCommand::Recover),
        ] {
            let cli = Cli::try_parse_from(["maxonkey"].into_iter().chain(args)).unwrap();
            assert_eq!(cli.command, Some(Command::Vault(expected)));
        }
        let cli = Cli::try_parse_from(["maxonkey", "vault", "status", "--vault", "x.db"]).unwrap();
        assert_eq!(cli.vault, Some(PathBuf::from("x.db")));
    }

    #[test]
    fn parses_secret_commands() {
        let owned = |s: &str| s.to_owned();
        for (args, expected) in [
            (
                vec!["set", "shop", "prod", "STRIPE_KEY"],
                SecretCommand::Set {
                    project: owned("shop"),
                    environment: owned("prod"),
                    name: owned("STRIPE_KEY"),
                    stdin: false,
                },
            ),
            (
                vec!["set", "shop", "prod", "STRIPE_KEY", "--stdin"],
                SecretCommand::Set {
                    project: owned("shop"),
                    environment: owned("prod"),
                    name: owned("STRIPE_KEY"),
                    stdin: true,
                },
            ),
            (
                vec!["get", "shop", "prod", "STRIPE_KEY"],
                SecretCommand::Get {
                    project: owned("shop"),
                    environment: owned("prod"),
                    name: owned("STRIPE_KEY"),
                },
            ),
            (
                vec!["list"],
                SecretCommand::List {
                    project: None,
                    environment: None,
                },
            ),
            (
                vec!["list", "shop", "prod"],
                SecretCommand::List {
                    project: Some(owned("shop")),
                    environment: Some(owned("prod")),
                },
            ),
            (
                vec!["delete", "shop", "prod", "STRIPE_KEY", "--yes"],
                SecretCommand::Delete {
                    project: owned("shop"),
                    environment: owned("prod"),
                    name: owned("STRIPE_KEY"),
                    yes: true,
                },
            ),
        ] {
            let cli = Cli::try_parse_from(["maxonkey", "secret"].into_iter().chain(args)).unwrap();
            assert_eq!(cli.command, Some(Command::Secret(expected)));
        }
        // A value can never be given as an extra argument.
        for args in [
            vec!["secret", "set", "p", "e", "n", "MK-CANARY-value"],
            vec!["secret", "get", "p", "e"],
            vec!["secret", "list", "p", "e", "n"],
        ] {
            assert!(Cli::try_parse_from(["maxonkey"].into_iter().chain(args)).is_err());
        }
    }

    /// ST-11: there is no way to pass a credential or secret on the command line.
    #[test]
    fn credential_options_do_not_exist() {
        for option in [
            "--passphrase",
            "--password",
            "--value",
            "--key",
            "--recovery-key",
        ] {
            for command in [
                vec!["vault", "init"],
                vec!["vault", "change-passphrase"],
                vec!["vault", "recover"],
                vec!["secret", "set", "p", "e", "n"],
                vec!["secret", "get", "p", "e", "n"],
                vec!["secret", "list"],
                vec!["secret", "delete", "p", "e", "n"],
            ] {
                let args = ["maxonkey"]
                    .into_iter()
                    .chain(command)
                    .chain([option, "MK-CANARY-arg"]);
                assert_eq!(
                    Cli::try_parse_from(args).unwrap_err().kind(),
                    ErrorKind::UnknownArgument
                );
            }
        }
    }

    #[test]
    fn clap_handles_help_and_version_flags() {
        for (input, expected) in [
            ("help", ErrorKind::DisplayHelp),
            ("--help", ErrorKind::DisplayHelp),
            ("--version", ErrorKind::DisplayVersion),
        ] {
            assert_eq!(
                Cli::try_parse_from(["maxonkey", input]).unwrap_err().kind(),
                expected
            );
        }
    }
}
