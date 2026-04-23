use crate::config::AmdGpuLevel;
use crate::system::write_gpu_level;
use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "hoist-gpuctl", version)]
pub struct GpuCtlCli {
    #[command(subcommand)]
    pub command: GpuCtlCommand,
}

#[derive(Debug, Subcommand)]
pub enum GpuCtlCommand {
    Apply {
        #[arg(long)]
        card: u32,
        #[arg(long)]
        level: AmdGpuLevelArg,
    },
    Revert {
        #[arg(long)]
        card: u32,
        #[arg(long)]
        level: AmdGpuLevelArg,
    },
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum AmdGpuLevelArg {
    Auto,
    Low,
    High,
    Manual,
    ProfileStandard,
    ProfileMinSclk,
    ProfileMinMclk,
    ProfilePeak,
}

impl From<AmdGpuLevelArg> for AmdGpuLevel {
    fn from(value: AmdGpuLevelArg) -> Self {
        match value {
            AmdGpuLevelArg::Auto => Self::Auto,
            AmdGpuLevelArg::Low => Self::Low,
            AmdGpuLevelArg::High => Self::High,
            AmdGpuLevelArg::Manual => Self::Manual,
            AmdGpuLevelArg::ProfileStandard => Self::ProfileStandard,
            AmdGpuLevelArg::ProfileMinSclk => Self::ProfileMinSclk,
            AmdGpuLevelArg::ProfileMinMclk => Self::ProfileMinMclk,
            AmdGpuLevelArg::ProfilePeak => Self::ProfilePeak,
        }
    }
}

pub fn run_gpuctl(cmd: GpuCtlCommand) -> Result<()> {
    ensure_root("hoist-gpuctl")?;
    match cmd {
        GpuCtlCommand::Apply { card, level } | GpuCtlCommand::Revert { card, level } => {
            write_gpu_level(card, &level.into())?;
        }
    }
    Ok(())
}

fn ensure_root(helper_name: &str) -> Result<()> {
    if nix::unistd::Uid::effective().is_root() {
        Ok(())
    } else {
        bail!("{helper_name} must run as root")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn gpuctl_validation_rejects_unknown_subcmd() {
        let bad = GpuCtlCli::try_parse_from(["hoist-gpuctl", "unknown"]);
        assert!(bad.is_err());
    }
}
