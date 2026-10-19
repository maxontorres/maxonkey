use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "maxonkey",
    version,
    about = "MaxonKey — local-first secrets manager prototype"
)]
#[command(
    after_help = "Run without a command to launch the TUI. Passphrases are read only from the terminal.\nStoring secrets is not implemented yet."
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
