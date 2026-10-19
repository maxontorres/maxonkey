use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "maxonkey",
    version,
    about = "MaxonKey — local-first secrets manager prototype"
)]
#[command(
    after_help = "Run without a command to launch the TUI. No secret management is implemented yet."
)]
pub struct Cli {
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
