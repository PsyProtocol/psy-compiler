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
    let shell = parse_shell(&command.shell)?;
    clap_complete::generate(shell, &mut DargoCli::command(), "dargo", &mut std::io::stdout());
    Ok(())
}

fn parse_shell(shell: &str) -> Result<Shell, CliError> {
    match shell.to_lowercase().as_str() {
        "bash" => Ok(Shell::Bash),
        "elvish" => Ok(Shell::Elvish),
        "fish" => Ok(Shell::Fish),
        "powershell" => Ok(Shell::PowerShell),
        "zsh" => Ok(Shell::Zsh),
        _ => {
            return Err(CliError::Generic(
                "Invalid shell. Supported shells are: bash, elvish, fish, powershell, zsh".to_string(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use clap_complete::Shell;

    use super::{parse_shell, run, CompleteCommand};

    #[test]
    fn shell_parser_accepts_supported_names_case_insensitively() {
        for (name, expected) in [("bash", Shell::Bash), ("ELVISH", Shell::Elvish), ("Fish", Shell::Fish), ("powershell", Shell::PowerShell), ("zsh", Shell::Zsh)] {
            assert_eq!(parse_shell(name).unwrap(), expected);
        }
    }

    #[test]
    fn shell_parser_rejects_unknown_names() {
        let error = parse_shell("cmd").expect_err("unsupported shell must be rejected");
        assert!(error.to_string().contains("Supported shells"));
    }

    #[test]
    fn run_emits_the_completion_script_for_a_supported_shell() {
        run(CompleteCommand { shell: "bash".to_string() })
            .expect("completion generation must succeed for a supported shell");
    }
}
