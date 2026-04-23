use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "hoist",
    version,
    about = "Temporary Linux performance tweaks for one command"
)]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub profile: Option<String>,
    #[command(subcommand)]
    pub subcommand: Option<HoistSubcommand>,
    #[arg(last = true, trailing_var_arg = true)]
    pub command: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum HoistSubcommand {
    ValidateConfig {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    PrintConfigPaths,
    Inspect,
    HelperInfo,
    /// Remove orphaned state files left by SIGKILL'd hoist processes.
    Cleanup,
}
