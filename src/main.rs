mod analysis;
mod capture;
mod commands;
mod config;
mod db;
mod diff;
mod display;
mod scope;

use clap::{Parser, Subcommand, ValueEnum};
use std::process;

pub type AppError = Box<dyn std::error::Error + Send + Sync>;
pub type AppResult<T> = Result<T, AppError>;

#[derive(Parser)]
#[command(
    name = "harn",
    version,
    about = "Capture and improve your Claude Code harness"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Status {
        #[arg(long, value_enum, default_value_t = ScopeArg::Project)]
        scope: ScopeArg,
    },
    Analyze {
        #[arg(long, value_enum, default_value_t = ScopeArg::Both)]
        scope: ScopeArg,
    },
    Generate,
    Backfill {
        #[arg(long, default_value_t = 30)]
        days: i64,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Hook {
        #[command(subcommand)]
        event: HookCommand,
    },
}

#[derive(Subcommand, Clone)]
pub enum ConfigCommand {
    Set { key: String, value: String },
    Get { key: String },
    List,
    Path,
}

#[derive(Subcommand, Clone)]
pub enum HookCommand {
    Prompt,
    Tool,
    Stop,
    #[command(name = "session-end")]
    SessionEnd,
    #[command(name = "subagent-stop")]
    SubagentStop,
    #[command(name = "post-compact")]
    PostCompact,
    #[command(name = "task-completed")]
    TaskCompleted,
    Commit {
        commit_hash: String,
        branch: Option<String>,
        project_path: Option<String>,
    },
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ScopeArg {
    Project,
    User,
    Both,
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => commands::init::run(),
        Commands::Status { scope } => commands::status::run(scope.into()),
        Commands::Analyze { scope } => commands::analyze::run(scope.into()),
        Commands::Generate => commands::generate::run(),
        Commands::Backfill { days } => commands::backfill::run(days),
        Commands::Config { command } => commands::config::run(command),
        Commands::Hook { event } => {
            commands::hook::run(event);
            Ok(())
        }
    };

    if let Err(error) = result {
        eprintln!("harn: {error}");
        process::exit(1);
    }
}

pub fn boxed_error(message: impl Into<String>) -> AppError {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        message.into(),
    ))
}
