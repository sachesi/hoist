use crate::config::{AmdGpuLevel, GPUCTL_PATH};
use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

const HELPER_TIMEOUT: Duration = Duration::from_secs(8);
const TUNED_TIMEOUT: Duration = Duration::from_secs(8);
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub fn run_argv(argv: &[String]) -> Result<()> {
    let (bin, args) = argv.split_first().context("empty command")?;
    let status = Command::new(bin).args(args).status()?;
    if !status.success() {
        bail!("command {} exited with status {status}", bin);
    }
    Ok(())
}

pub fn run_shell(snippet: &str) -> Result<()> {
    let status = Command::new("/bin/sh").args(["-c", snippet]).status()?;
    if !status.success() {
        bail!("shell hook exited with status {status}");
    }
    Ok(())
}

#[derive(Debug)]
enum CommandRunError {
    Launch(std::io::Error),
    Timeout(Duration),
    Exit(ExitStatus),
}

fn run_command_with_timeout<I, S>(
    bin: &str,
    args: I,
    timeout: Duration,
) -> std::result::Result<(), CommandRunError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = Command::new(bin)
        .args(args)
        .spawn()
        .map_err(CommandRunError::Launch)?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                return Err(CommandRunError::Exit(status));
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(CommandRunError::Timeout(timeout));
                }
                std::thread::sleep(WAIT_POLL_INTERVAL);
            }
            Err(e) => return Err(CommandRunError::Launch(e)),
        }
    }
}

pub fn run_pkexec_gpuctl(args: &[&str]) -> Result<()> {
    run_pkexec(GPUCTL_PATH, args)
}

fn run_pkexec(helper_path: &str, args: &[&str]) -> Result<()> {
    let mut argv = vec![helper_path];
    argv.extend_from_slice(args);

    match run_command_with_timeout("pkexec", argv, HELPER_TIMEOUT) {
        Ok(()) => Ok(()),
        Err(CommandRunError::Launch(e)) => bail!("pkexec launch failed: {e}"),
        Err(CommandRunError::Timeout(timeout)) => {
            bail!("helper timed out after {}s", timeout.as_secs())
        }
        Err(CommandRunError::Exit(status)) => bail!("helper exited with status {status}"),
    }
}

pub fn tuned_active() -> Result<Option<String>> {
    let output = Command::new("tuned-adm").arg("active").output();
    let output = match output {
        Ok(o) => o,
        Err(_) => return Ok(None),
    };
    if !output.status.success() {
        return Ok(None);
    }
    Ok(parse_tuned_active(&String::from_utf8_lossy(&output.stdout)))
}

pub fn parse_tuned_active(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let needle = "Current active profile:";
        if let Some(rest) = line.trim().strip_prefix(needle) {
            let profile = rest.trim();
            if !profile.is_empty() {
                return Some(profile.to_string());
            }
        }
    }
    None
}

pub fn tuned_set_profile(profile: &str) -> Result<()> {
    match run_command_with_timeout("tuned-adm", ["profile", profile], TUNED_TIMEOUT) {
        Ok(()) => Ok(()),
        Err(CommandRunError::Launch(e)) => bail!("failed to launch tuned-adm: {e}"),
        Err(CommandRunError::Timeout(timeout)) => {
            bail!("tuned-adm timed out after {}s", timeout.as_secs())
        }
        Err(CommandRunError::Exit(status)) => {
            bail!("tuned-adm profile {profile} failed: {status}")
        }
    }
}

pub fn gpu_level_path(card: u32) -> PathBuf {
    Path::new("/sys/class/drm")
        .join(format!("card{card}"))
        .join("device/power_dpm_force_performance_level")
}

pub fn read_gpu_level(card: u32) -> Result<AmdGpuLevel> {
    let p = gpu_level_path(card);
    let raw = fs::read_to_string(&p).with_context(|| format!("unable to read {}", p.display()))?;
    AmdGpuLevel::parse_sysfs(&raw)
        .with_context(|| format!("unsupported amdgpu level '{}'", raw.trim()))
}

pub fn write_gpu_level(card: u32, level: &AmdGpuLevel) -> Result<()> {
    let p = gpu_level_path(card);
    fs::write(&p, format!("{}\n", level.as_str()))
        .with_context(|| format!("unable to write {}", p.display()))?;
    let now = read_gpu_level(card)?;
    if &now != level {
        bail!(
            "failed to verify amdgpu level on card{card}: expected {}, got {}",
            level.as_str(),
            now.as_str()
        );
    }
    Ok(())
}

pub fn gpuctl_exists() -> bool {
    Path::new(GPUCTL_PATH).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tuned_output() {
        let p = parse_tuned_active("foo\nCurrent active profile: balanced\n");
        assert_eq!(p.as_deref(), Some("balanced"));
    }

    #[test]
    fn gpu_path() {
        assert_eq!(
            gpu_level_path(1).display().to_string(),
            "/sys/class/drm/card1/device/power_dpm_force_performance_level"
        );
    }

    #[test]
    fn command_timeout_returns_timeout_error() {
        let err = run_command_with_timeout("sh", ["-c", "sleep 1"], Duration::from_millis(50))
            .expect_err("must timeout");

        assert!(matches!(err, CommandRunError::Timeout(_)));
    }

    #[test]
    fn command_nonzero_exit_returns_exit_error() {
        let err = run_command_with_timeout("sh", ["-c", "exit 3"], Duration::from_secs(1))
            .expect_err("must fail");

        assert!(matches!(err, CommandRunError::Exit(_)));
    }
}
