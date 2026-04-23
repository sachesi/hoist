use anyhow::{Context, Result, bail};
use nix::unistd::{User, getuid};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const SYSTEM_CONFIG: &str = "/etc/hoist/default.toml";
pub const GPUCTL_PATH: &str = "/usr/libexec/hoist-gpuctl";

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    pub system: PathBuf,
    pub user: PathBuf,
    pub selected: PathBuf,
    pub system_exists: bool,
    pub user_exists: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub global: Global,
    pub profile: BTreeMap<String, Profile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Global {
    pub poll_interval_ms: Option<u64>,
    pub require_all: bool,
    pub log_level: Option<String>,
    pub default_profile: String,
    pub helper_timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub cpu: Option<CpuConfig>,
    pub gpu: Option<GpuConfig>,
    pub process: Option<ProcessConfig>,
    pub hooks: Option<Hooks>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CpuConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    pub enter_profile: String,
    pub restore_previous: bool,
    pub restore_to_profile: Option<String>,
    pub fallback_exit_profile: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GpuConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    pub kind: GpuKind,
    pub card: u32,
    pub enter_level: AmdGpuLevel,
    pub restore_previous: bool,
    pub restore_to_level: Option<AmdGpuLevel>,
    pub fallback_exit_level: Option<AmdGpuLevel>,
}

impl CpuConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

impl GpuConfig {
    pub fn enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GpuKind {
    Amdgpu,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AmdGpuLevel {
    Auto,
    Low,
    High,
    Manual,
    ProfileStandard,
    ProfileMinSclk,
    ProfileMinMclk,
    ProfilePeak,
}

impl AmdGpuLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Low => "low",
            Self::High => "high",
            Self::Manual => "manual",
            Self::ProfileStandard => "profile_standard",
            Self::ProfileMinSclk => "profile_min_sclk",
            Self::ProfileMinMclk => "profile_min_mclk",
            Self::ProfilePeak => "profile_peak",
        }
    }

    pub fn parse_sysfs(s: &str) -> Option<Self> {
        match s.trim() {
            "auto" => Some(Self::Auto),
            "low" => Some(Self::Low),
            "high" => Some(Self::High),
            "manual" => Some(Self::Manual),
            "profile_standard" => Some(Self::ProfileStandard),
            "profile_min_sclk" => Some(Self::ProfileMinSclk),
            "profile_min_mclk" => Some(Self::ProfileMinMclk),
            "profile_peak" => Some(Self::ProfilePeak),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessConfig {
    pub nice: i32,
    pub renice_descendants: bool,
    pub renice_interval_ms: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Hooks {
    #[serde(default)]
    pub start: Vec<Vec<String>>,
    #[serde(default)]
    pub stop: Vec<Vec<String>>,
    #[serde(default)]
    pub start_sh: Vec<String>,
    #[serde(default)]
    pub stop_sh: Vec<String>,
}

pub fn user_config_path() -> Result<PathBuf> {
    let uid = getuid();
    let user = User::from_uid(uid)
        .context("failed to query current user")?
        .with_context(|| format!("unable to resolve uid {}", uid.as_raw()))?;
    Ok(Path::new(&user.dir).join(".config/hoist/default.toml"))
}

pub fn resolve_config_path(explicit: Option<&Path>) -> Result<ConfigPaths> {
    let system = PathBuf::from(SYSTEM_CONFIG);
    let user = user_config_path()?;
    Ok(resolve_from_paths(system, user, explicit))
}

fn resolve_from_paths(system: PathBuf, user: PathBuf, explicit: Option<&Path>) -> ConfigPaths {
    let system_exists = system.exists();
    let user_exists = user.exists();
    let selected = match explicit {
        Some(p) => p.to_path_buf(),
        None if user_exists => user.clone(),
        None => system.clone(),
    };
    ConfigPaths {
        system,
        user,
        selected,
        system_exists,
        user_exists,
    }
}

pub fn load_config(explicit: Option<&Path>) -> Result<(ConfigPaths, Config)> {
    let paths = resolve_config_path(explicit)?;
    let raw = fs::read_to_string(&paths.selected)
        .with_context(|| format!("unable to read config {}", paths.selected.display()))?;
    let cfg: Config = toml::from_str(&raw)
        .with_context(|| format!("invalid TOML in {}", paths.selected.display()))?;
    validate_config(&cfg)?;
    Ok((paths, cfg))
}

pub fn validate_config(cfg: &Config) -> Result<()> {
    if !cfg.profile.contains_key(&cfg.global.default_profile) {
        bail!(
            "global.default_profile '{}' is missing from [profile]",
            cfg.global.default_profile
        );
    }
    if let Some(ms) = cfg.global.poll_interval_ms
        && ms == 0
    {
        bail!("global.poll_interval_ms must be greater than 0");
    }
    for (name, profile) in &cfg.profile {
        validate_profile_name(name)?;
        if let Some(proc_cfg) = &profile.process
            && !(-20..=19).contains(&proc_cfg.nice)
        {
            bail!("profile.{name}.process.nice must be between -20 and 19");
        }
        if let Some(proc_cfg) = &profile.process
            && proc_cfg.renice_descendants
            && proc_cfg.renice_interval_ms == 0
        {
            bail!("profile.{name}.process.renice_interval_ms must be greater than 0");
        }
        if let Some(h) = &profile.hooks {
            validate_hooks(name, "hooks", h)?;
        }
    }
    Ok(())
}

fn validate_hooks(profile: &str, kind: &str, hooks: &Hooks) -> Result<()> {
    for (phase, cmds) in [("start", &hooks.start), ("stop", &hooks.stop)] {
        for (i, cmd) in cmds.iter().enumerate() {
            if cmd.is_empty() {
                bail!("profile.{profile}.{kind}.{phase}[{i}] must not be empty");
            }
            let p = Path::new(&cmd[0]);
            if !p.is_absolute() {
                bail!("profile.{profile}.{kind}.{phase}[{i}][0] must be an absolute path");
            }
        }
    }
    for (phase, cmds) in [("start_sh", &hooks.start_sh), ("stop_sh", &hooks.stop_sh)] {
        for (i, cmd) in cmds.iter().enumerate() {
            if cmd.trim().is_empty() {
                bail!("profile.{profile}.{kind}.{phase}[{i}] must not be empty");
            }
        }
    }
    Ok(())
}

pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("invalid profile name '{name}'")
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_config_ok() {
        let raw = r#"
[global]
poll_interval_ms = 700
require_all = false
default_profile = "default"

[profile.default.process]
nice = -10
renice_descendants = true
renice_interval_ms = 700
"#;
        let cfg: Config = toml::from_str(raw).expect("parse");
        validate_config(&cfg).expect("validate");
    }

    #[test]
    fn rejects_zero_poll_interval() {
        let raw = r#"
[global]
poll_interval_ms = 0
require_all = false
default_profile = "default"

[profile.default.process]
nice = 0
renice_descendants = false
renice_interval_ms = 700
"#;
        let cfg: Config = toml::from_str(raw).expect("parse");
        let err = validate_config(&cfg).expect_err("must reject");
        assert!(err.to_string().contains("poll_interval_ms"));
    }

    #[test]
    fn rejects_zero_renice_interval_when_enabled() {
        let raw = r#"
[global]
poll_interval_ms = 700
require_all = false
default_profile = "default"

[profile.default.process]
nice = 0
renice_descendants = true
renice_interval_ms = 0
"#;
        let cfg: Config = toml::from_str(raw).expect("parse");
        let err = validate_config(&cfg).expect_err("must reject");
        assert!(err.to_string().contains("renice_interval_ms"));
    }

    #[test]
    fn accepts_inline_shell_hooks() {
        let raw = r#"
[global]
poll_interval_ms = 700
require_all = false
default_profile = "default"

[profile.default.hooks]
start = []
stop = []
start_sh = ["echo start"]
stop_sh = ["echo stop"]
"#;
        let cfg: Config = toml::from_str(raw).expect("parse");
        validate_config(&cfg).expect("validate");
    }

    #[test]
    fn rejects_empty_inline_shell_hook() {
        let raw = r#"
[global]
poll_interval_ms = 700
require_all = false
default_profile = "default"

[profile.default.hooks]
start = []
stop = []
start_sh = ["  "]
stop_sh = []
"#;
        let cfg: Config = toml::from_str(raw).expect("parse");
        let err = validate_config(&cfg).expect_err("must reject");
        assert!(err.to_string().contains("start_sh"));
    }

    #[test]
    fn cpu_enabled_defaults_true_when_omitted() {
        let raw = r#"
[global]
require_all = false
default_profile = "default"

[profile.default.cpu]
enter_profile = "throughput-performance"
restore_previous = true
"#;
        let cfg: Config = toml::from_str(raw).expect("parse");
        let cpu = cfg.profile["default"].cpu.as_ref().expect("cpu");
        assert!(cpu.enabled());
    }

    #[test]
    fn gpu_enabled_defaults_false_when_omitted() {
        let raw = r#"
[global]
require_all = false
default_profile = "default"

[profile.default.gpu]
kind = "amdgpu"
card = 1
enter_level = "high"
restore_previous = true
"#;
        let cfg: Config = toml::from_str(raw).expect("parse");
        let gpu = cfg.profile["default"].gpu.as_ref().expect("gpu");
        assert!(!gpu.enabled());
    }
}
