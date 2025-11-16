mod cli;
pub mod errors;

use psy_common::setup_logging;

const PANIC_MESSAGE: &str = "Program panic. This is a bug to be fixed.";

#[tokio::main]
async fn main() {
    setup_logging().ok();

    // Register a panic hook to display more readable panic messages to end-users
    let (panic_hook, _) = color_eyre::config::HookBuilder::default()
        .display_env_section(false)
        .panic_section(PANIC_MESSAGE)
        .into_hooks();
    panic_hook.install();

    if let Err(report) = cli::start_cli().await {
        eprintln!("{report:#}");
        std::process::exit(1);
    }
}
