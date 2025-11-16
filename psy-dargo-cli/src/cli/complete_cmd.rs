use clap::{Args, CommandFactory};
use clap_complete::Shell;

use crate::{cli::DargoCli, errors::CliError};

/// Generates a shell completion script for your favorite shell
#[derive(Debug, Clone, Args)]
pub(crate) struct CompleteCommand {
    /// The shell to generate completions for. possible value: bash, elvish,
    /// fish, powershell, zsh
    pub(crate) shell: String,
}

pub(crate) fn run(command: CompleteCommand) -> Result<(), CliError> {
    let shell = match command.shell.to_lowercase().as_str() {
        "bash" => Shell::Bash,
        "elvish" => Shell::Elvish,
        "fish" => Shell::Fish,
        "powershell" => Shell::PowerShell,
        "zsh" => Shell::Zsh,
        _ => {
            return Err(CliError::Generic(
                "Invalid shell. Supported shells are: bash, elvish, fish, powershell, zsh".to_string(),
            ));
        }
    };
    clap_complete::generate(shell, &mut DargoCli::command(), "dargo", &mut std::io::stdout());
    Ok(())
}
